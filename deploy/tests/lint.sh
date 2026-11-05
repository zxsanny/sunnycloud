#!/usr/bin/env bash
# AC-1 (FROM digests, --locked) and AC-6 (declared health checks are the documented ones, none always-true).
# Needs: docker compose, python3.
set -euo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

DOCKERFILES=("$REPO_ROOT"/deploy/docker/*.Dockerfile)
COMPOSE="$REPO_ROOT/deploy/compose/compose.dev.yml"

# AC-1: every FROM names a digest (stage aliases of earlier stages are not images)
for f in "${DOCKERFILES[@]}" "$REPO_ROOT"/deploy/tests/*.Dockerfile; do
  stages=" "
  while IFS= read -r line; do
    img="$(awk '{print $2}' <<<"$line")"
    alias="$(awk 'tolower($3)=="as"{print $4}' <<<"$line")"
    if [[ "$stages" == *" $img "* ]]; then :
    elif [[ "$img" == *@sha256:* ]]; then pass "FROM digest: $(basename "$f"): $img"
    else fail "FROM without digest: $(basename "$f"): $line"; fi
    if [[ -n "$alias" ]]; then stages="$stages$alias "; fi
  done < <(grep -E '^FROM ' "$f")
done

# AC-1: the cargo steps are --locked
for f in "${DOCKERFILES[@]}"; do
  while IFS= read -r line; do
    if [[ "$line" == *--locked* ]]; then pass "--locked: $(basename "$f")"; else fail "cargo step without --locked: $(basename "$f"): $line"; fi
  done < <(grep -E 'RUN cargo (build|chef cook)' "$f")
done

# AC-6: no always-true health check in Dockerfiles or compose
ALWAYS_TRUE='(\|\|[[:space:]]*(true|:|exit[[:space:]]+0))|(^|[^[:alnum:]_-])(exit[[:space:]]+0)|"(true|:|/bin/true)"|CMD[[:space:]]+(true|:|/bin/true)'
for f in "${DOCKERFILES[@]}" "$COMPOSE"; do
  if grep -n -E -i 'healthcheck|test:' "$f" | grep -E "$ALWAYS_TRUE"; then fail "always-true health check in $(basename "$f")"; else pass "no always-true health check: $(basename "$f")"; fi
done
if grep -n -E "^[[:space:]]*(HEALTHCHECK[[:space:]]+NONE)" "${DOCKERFILES[@]}"; then fail "HEALTHCHECK NONE"; fi

# AC-6: declared commands are exactly the documented ones (compose view)
docker compose -p lint -f "$COMPOSE" config --format json | python3 -I -c '
import json, sys
cfg = json.load(sys.stdin)["services"]
want = {
    "api": ["CMD", "sunnycloud", "healthcheck"],
    "worker": ["CMD", "sunnycloud-worker", "healthcheck"],
    "decoder-interactive": ["CMD", "decoder-runner", "health"],
    "decoder-general": ["CMD", "decoder-runner", "health"],
}
bad = 0
for name, cmd in want.items():
    got = cfg[name]["healthcheck"]["test"]
    ok = got == cmd
    print(("PASS" if ok else "FAIL"), "compose health check", name, got)
    bad += 0 if ok else 1
sys.exit(1 if bad else 0)
' || fail "compose health checks differ from the documented commands"

# AC-6: declared commands in the Dockerfiles
expect() { # file, exact HEALTHCHECK line suffix
  if grep -q -F "$2" "$REPO_ROOT/deploy/docker/$1"; then pass "$1 declares $2"; else fail "$1 does not declare $2"; fi
}
expect api.Dockerfile 'HEALTHCHECK --start-period=120s CMD ["sunnycloud", "healthcheck"]'
expect worker.Dockerfile 'HEALTHCHECK CMD ["sunnycloud-worker", "healthcheck"]'
expect decoder.Dockerfile 'HEALTHCHECK CMD ["decoder-runner", "health"]'
finish
