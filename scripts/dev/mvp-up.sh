#!/usr/bin/env bash
# Starts the MVP stack: Postgres and Caddy in Docker, then migrate, key check, and api, worker and the two
# decoders as host processes from release binaries. Safe to run again: running parts are left alone.
# Runbook: _docs/04_deploy/mvp_runbook.md
set -euo pipefail
# shellcheck source=scripts/dev/mvp-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/mvp-lib.sh"

# The server's own floor is blob_free_reserve; this stops a start on a nearly full disk long before it.
MIN_FREE_GIB="${MVP_MIN_FREE_GIB:-50}"

"$REPO_ROOT/scripts/dev/mvp-secrets.sh"
load_env
require_systemd_user

free_gib=$(( $(df --output=avail -B1G "$MVP_DATA" | tail -n 1) ))
[ "$free_gib" -ge "$MIN_FREE_GIB" ] || die "only ${free_gib} GiB free on $MVP_DATA, need $MIN_FREE_GIB (MVP_MIN_FREE_GIB)"
log "free space on $MVP_DATA: ${free_gib} GiB"

# Build in the background and report every 30 s: a cold build takes minutes.
build() {
  local build_log="$MVP_RUN/build.log" started waited=0 pid status=0
  started=$(date +%s)
  (
    cd "$REPO_ROOT/src"
    export CARGO_TARGET_DIR="$MVP_TARGET" CARGO_INCREMENTAL=0 RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-1.97.1}"
    exec cargo build --release --locked -p sunnycloud-server -p sunnycloud-worker -p decoder-runner
  ) > "$build_log" 2>&1 &
  pid=$!
  while [ -d "/proc/$pid" ]; do
    sleep 1
    waited=$((waited + 1))
    if [ $((waited % 30)) -eq 0 ]; then
      log "build running $(($(date +%s) - started)) s: $(tail -n 1 "$build_log")"
    fi
  done
  wait "$pid" || status=$?
  if [ "$status" -ne 0 ]; then
    tail -n 30 "$build_log" >&2
    die "build failed (exit $status), full log $build_log"
  fi
  log "build done in $(($(date +%s) - started)) s: $(tail -n 1 "$build_log")"
}
if [ "${MVP_SKIP_BUILD:-}" = 1 ]; then
  log "MVP_SKIP_BUILD=1: using the binaries in $BIN_DIR"
else
  build
fi
for tool in "${MEDIA_TOOLS[@]}"; do
  PATH="$SUPERVISED_PATH" command -v "$tool" > /dev/null || die "$tool is not on $SUPERVISED_PATH; the decoder refuses to start without it"
done

# The web bundle is built by its own package; until it exists the placeholder page is served.
if [ -z "${WEB_DIST:-}" ]; then
  if [ -f "$REPO_ROOT/src/web/dist/index.html" ]; then
    WEB_DIST="$REPO_ROOT/src/web/dist"
  else
    WEB_DIST="$REPO_ROOT/deploy/mvp/web-placeholder"
    log "src/web/dist has no index.html: serving the placeholder page"
  fi
fi
export WEB_DIST

if is_placeholder "$MVP_SECRETS/acme_email"; then
  ACME_EMAIL_LINE=""
  log "acme_email is a placeholder: the ACME account is registered without an email"
else
  ACME_EMAIL_LINE="email $(tr -d '\r\n' < "$MVP_SECRETS/acme_email")"
fi
export ACME_EMAIL_LINE
log "ACME CA: ${ACME_CA:-Lets Encrypt STAGING (default)}"

log "starting postgres and caddy"
compose up -d --wait db caddy

sunnycloud="$(binary_of api)"
"$sunnycloud" migrate
"$sunnycloud" admin keycheck-init

for name in "${PROCESSES[@]}"; do
  start_process "$name"
done
wait_http "api ops" "http://127.0.0.1:9090/readyz"
wait_http "worker ops" "http://127.0.0.1:9092/readyz"
wait_http "public server-info" "https://$PUBLIC_HOST/api/v1/server-info" "--insecure"

for name in oidc_client_id provider_oidc_client_secret; do
  if is_placeholder "$MVP_SECRETS/$name"; then
    log "WARNING: $name is a placeholder: Google sign-in will not work until the real value is in secrets-dev/$name"
  fi
done
log "up. Status: scripts/dev/mvp-status.sh"
