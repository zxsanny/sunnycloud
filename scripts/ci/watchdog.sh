#!/usr/bin/env bash
# Runs a command and never lets it hang silently: prints a heartbeat every WATCHDOG_TICK seconds
# (elapsed, state, output growth) and, when the command prints nothing for WATCHDOG_STALL seconds,
# reports STALLED, kills the whole process group and exits 124.
# usage: watchdog.sh [--name LABEL] -- COMMAND [ARGS...]      (the "--" is optional)
# Exit code: the command's own code, 124 on STALLED.
# Defaults: WATCHDOG_TICK=30, WATCHDOG_STALL=120 (seconds, integers). The environment variables exist for the fast test only.
set -uo pipefail

tick="${WATCHDOG_TICK:-30}"
stall="${WATCHDOG_STALL:-120}"
kill_grace="${WATCHDOG_KILL_GRACE:-5}"
name=""
tag="[watchdog]"

while [ $# -gt 0 ]; do
  case "$1" in
    --name) name="${2:?--name needs a value}"; shift 2 ;;
    --) shift; break ;;
    *) break ;;
  esac
done
if [ $# -eq 0 ]; then
  echo "$tag usage: watchdog.sh [--name LABEL] -- COMMAND [ARGS...]" >&2
  exit 2
fi
for value in "$tick" "$stall" "$kill_grace"; do
  case "$value" in
    '' | *[!0-9]* | 0) echo "$tag FAILED: WATCHDOG_TICK, WATCHDOG_STALL and WATCHDOG_KILL_GRACE must be positive integers" >&2; exit 2 ;;
  esac
done
[ -n "$name" ] || name="$(basename "$1")"

log="$(mktemp)"
tail_pid=""
alive() { ps -p "$1" >/dev/null; }
cleanup() {
  if [ -n "$tail_pid" ] && alive "$tail_pid"; then kill "$tail_pid" || echo "$tag could not stop the output follower $tail_pid" >&2; fi
  rm -f "$log"
}
trap cleanup EXIT

echo "$tag $name: start, heartbeat every ${tick}s, silence limit ${stall}s"

# The command gets its own session so a kill reaches every child it spawned (pid == process group id).
setsid "$@" </dev/null >"$log" 2>&1 &
cmd_pid=$!
tail -n +1 -f "$log" --pid="$cmd_pid" &
tail_pid=$!

start=$SECONDS
prev_size=0
last_beat_size=0
last_change=$SECONDS
idle=0
next_beat=$((SECONDS + tick))
stalled=0

while alive "$cmd_pid"; do
  sleep 1
  size="$(stat -c %s "$log")"
  if [ "$size" -ne "$prev_size" ]; then last_change=$SECONDS; fi
  idle=$((SECONDS - last_change))
  prev_size="$size"
  if [ "$SECONDS" -ge "$next_beat" ]; then
    growth=$((size - last_beat_size))
    if [ "$growth" -gt 0 ]; then state="PROGRESSING"; else state="QUIET"; fi
    echo "$tag $name: elapsed=$((SECONDS - start))s state=$state output=+${growth}B total=${size}B idle=${idle}s"
    last_beat_size="$size"
    next_beat=$((SECONDS + tick))
  fi
  if [ "$idle" -ge "$stall" ]; then
    stalled=1
    break
  fi
done

if [ "$stalled" -eq 1 ]; then
  echo "$tag $name: STALLED elapsed=$((SECONDS - start))s no output for ${idle}s (limit ${stall}s), aborting process group $cmd_pid"
  kill -TERM -- "-$cmd_pid" || echo "$tag $name: process group $cmd_pid was already gone"
  for _ in $(seq 1 "$kill_grace"); do
    alive "$cmd_pid" || break
    sleep 1
  done
  if alive "$cmd_pid"; then
    echo "$tag $name: still alive after ${kill_grace}s, sending SIGKILL to the process group"
    kill -KILL -- "-$cmd_pid" || echo "$tag $name: process group $cmd_pid disappeared before SIGKILL"
  fi
  wait "$cmd_pid" || true
  wait "$tail_pid" || true
  tail_pid=""
  echo "$tag $name: FAILED STALLED (exit 124)"
  exit 124
fi

wait "$cmd_pid"
rc=$?
wait "$tail_pid" || true
tail_pid=""
if [ "$rc" -eq 0 ]; then
  echo "$tag $name: DONE elapsed=$((SECONDS - start))s exit=0"
else
  echo "$tag $name: FAILED elapsed=$((SECONDS - start))s exit=$rc"
fi
exit "$rc"
