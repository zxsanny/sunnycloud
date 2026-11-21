#!/usr/bin/env bash
# Starts the isolated acceptance stack: a throwaway Postgres container (database `acceptance`), the mock IdP,
# api, worker, two decoder lanes and a one-origin web server for the SPA and the API. Everything listens on
# 127.0.0.1 only. Runbook: _docs/04_deploy/acceptance.md. Safe to run twice: running parts are left alone.
# ACC_SKIP_BUILD=1 reuses the binaries and the web bundle built earlier.
set -euo pipefail
# shellcheck source=acceptance-lib.sh
# shellcheck source=acceptance-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/acceptance-lib.sh"

MIN_FREE_GIB="${ACC_MIN_FREE_GIB:-10}"

require_systemd_user
command -v docker > /dev/null || die "docker is not installed"
[ -x "$BUN" ] || die "bun not found (set BUN=/path/to/bun)"
for tool in "${MEDIA_TOOLS[@]}"; do
  PATH="$SUPERVISED_PATH" command -v "$tool" > /dev/null || die "$tool is not on $SUPERVISED_PATH; the decoder refuses to start without it"
done

# A port taken by anything but this stack's own units means the stack cannot start (or would hit another one).
if ! unit_active api && ! db_running; then
  for port in "${ACC_PORTS[@]}"; do
    ss -Hltn "sport = :$port" | grep -q . && die "port $port is in use; pick another range with ACC_PORT_BASE (now $ACC_PORT_BASE, 7 consecutive ports)"
  done
fi

mkdir -p "$ACC_DATA" "$ACC_RESULTS"
free_gib=$(( $(df --output=avail -B1G "$ACC_DATA" | tail -n 1) ))
[ "$free_gib" -ge "$MIN_FREE_GIB" ] || die "only ${free_gib} GiB free on $ACC_DATA, need $MIN_FREE_GIB (ACC_MIN_FREE_GIB)"

# Secrets: the MVP generator with the acceptance directories (it never overwrites a file), then the files that
# differ: development settings, the mock IdP's client id and a client secret that exists only here.
MVP_DB_PORT="$ACC_DB_PORT" "$REPO_ROOT/scripts/dev/mvp-secrets.sh" > /dev/null
# The generator names the database `sunnycloud`; this stack's is `acceptance`.
sed -i 's#/sunnycloud$#/acceptance#' "$MVP_SECRETS/db_api_url" "$MVP_SECRETS/db_migrator_url"
write_env_file
printf '%s' "$ACC_CLIENT_ID" > "$MVP_SECRETS/oidc_client_id"
if [ ! -s "$MVP_SECRETS/provider_oidc_client_secret" ] || is_placeholder "$MVP_SECRETS/provider_oidc_client_secret"; then
  openssl rand -hex 24 > "$MVP_SECRETS/provider_oidc_client_secret"
fi
chmod 600 "$MVP_SECRETS/oidc_client_id" "$MVP_SECRETS/provider_oidc_client_secret"
write_acceptance_env
load_env

# Build the three binaries and the SPA in the background and report every 30 s.
run_watched() {
  local label="$1" build_log="$2" started waited=0 pid status=0
  shift 2
  started=$(date +%s)
  "$@" > "$build_log" 2>&1 &
  pid=$!
  while [ -d "/proc/$pid" ]; do
    sleep 1
    waited=$((waited + 1))
    [ $((waited % 30)) -ne 0 ] || log "$label running $(($(date +%s) - started)) s: $(tail -n 1 "$build_log")"
  done
  wait "$pid" || status=$?
  if [ "$status" -ne 0 ]; then
    tail -n 30 "$build_log" >&2
    die "$label failed (exit $status), full log $build_log"
  fi
  log "$label done in $(($(date +%s) - started)) s: $(tail -n 1 "$build_log")"
}

build_binaries() {
  cd "$REPO_ROOT/src"
  export CARGO_INCREMENTAL=0 RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-1.97.1}"
  exec cargo build --release --locked -p sunnycloud-server -p sunnycloud-worker -p decoder-runner
}

# The SPA is built from a copy: src/web is never written to.
build_web() {
  [ -f "$REPO_ROOT/src/web/wasm/pkg/sync_core_wasm.js" ] || die "src/web/wasm/pkg is missing: run the web package's build:wasm first"
  mkdir -p "$ACC_WEB_SRC"
  rsync -a --delete --exclude node_modules --exclude dist --exclude .env "$REPO_ROOT/src/web/" "$ACC_WEB_SRC/"
  cd "$ACC_WEB_SRC"
  "$BUN" install --frozen-lockfile
  "$BUN" run build
}

if [ "${ACC_SKIP_BUILD:-}" = 1 ]; then
  log "ACC_SKIP_BUILD=1: using the binaries in $BIN_DIR and the bundle in $ACC_WEB_DIST"
else
  run_watched "cargo build" "$MVP_RUN/build.log" build_binaries
  run_watched "web build" "$MVP_RUN/web-build.log" build_web
fi
[ -f "$ACC_WEB_DIST/index.html" ] || die "$ACC_WEB_DIST/index.html is missing"

# The database: a container of its own on tmpfs, so removing it removes the data. The image is the MVP's.
start_db() {
  if db_running; then
    log "database container already running"
    return 0
  fi
  docker rm -f -v "$ACC_DB_CONTAINER" > /dev/null 2>&1 || true
  local image
  image="$(grep -o 'postgres:18-trixie@sha256:[0-9a-f]*' "$REPO_ROOT/deploy/compose/compose.mvp.yml" | head -n 1)"
  [ -n "$image" ] || die "no postgres image reference in compose.mvp.yml"
  docker run -d --name "$ACC_DB_CONTAINER" --restart no --shm-size 256m --stop-timeout 30 \
    --label sunnycloud.acceptance=1 \
    -e POSTGRES_DB=acceptance -e POSTGRES_PASSWORD_FILE=/run/secrets/postgres_password \
    -v "$MVP_SECRETS/postgres_password:/run/secrets/postgres_password:ro" \
    -v "$MVP_SECRETS/db_migrator_url:/run/secrets/db_migrator_url:ro" \
    -v "$MVP_SECRETS/db_api_url:/run/secrets/db_api_url:ro" \
    -v "$REPO_ROOT/deploy/postgres/initdb.d:/docker-entrypoint-initdb.d:ro" \
    --tmpfs /var/lib/postgresql:rw,size=2g \
    -p "127.0.0.1:$ACC_DB_PORT:5432" \
    "$image" postgres -c max_connections=100 > /dev/null
  # TCP, not the socket: the init phase listens on the socket only.
  local waited=0
  until docker exec "$ACC_DB_CONTAINER" pg_isready -q -h 127.0.0.1 -U sunnycloud -d acceptance; do
    waited=$((waited + 1))
    [ "$waited" -le 90 ] || die "the database did not become ready in 90 s: docker logs $ACC_DB_CONTAINER"
    sleep 1
  done
  log "database acceptance is up in $ACC_DB_CONTAINER (127.0.0.1:$ACC_DB_PORT)"
}
log "starting the database"
start_db

sunnycloud="$(binary_of api)"
"$sunnycloud" migrate
"$sunnycloud" admin keycheck-init

IDP_SECRET="$(tr -d '\r\n' < "$MVP_SECRETS/provider_oidc_client_secret")"
start_aux idp "$REPO_ROOT/tests/mocks/idp" "MOCK_IDP_PORT=$ACC_IDP_PORT" "MOCK_IDP_CLIENT_ID=$ACC_CLIENT_ID" \
  "MOCK_IDP_CLIENT_SECRET=$IDP_SECRET" "MOCK_IDP_REDIRECT_URIS=$ACC_REDIRECT_URI" -- "$BUN" src/main.ts
unset IDP_SECRET
wait_http "mock idp" "$ACC_ISSUER/.well-known/openid-configuration"

for name in "${PROCESSES[@]}"; do
  start_process "$name"
done
wait_http "api ops" "http://127.0.0.1:$ACC_OPS_PORT/readyz"
wait_http "worker ops" "http://127.0.0.1:$ACC_WORKER_OPS_PORT/readyz"

start_aux web "$REPO_ROOT/tests/e2e" "WEB_PORT=$ACC_WEB_PORT" "API_URL=http://127.0.0.1:$ACC_API_PORT" \
  "WEB_DIST=$ACC_WEB_DIST" -- "$BUN" server/web-server.ts
wait_http "web server" "$ACC_ORIGIN/__acceptance/health"
wait_http "server-info through the web server" "$ACC_ORIGIN/api/v1/server-info"

log "up at $ACC_ORIGIN (mock IdP $ACC_ISSUER). Status: scripts/dev/acceptance-status.sh, remove: scripts/dev/acceptance-down.sh"
