#!/usr/bin/env bash
# AC-4 at the real defaults: a step that prints one line and then sleeps silently for 150 s must show
# a heartbeat line at least every 30 s, be reported STALLED after 120 s of silence, be aborted, and fail the job.
# Takes about 2 minutes. No environment overrides are used.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tag="[test-watchdog-realtime]"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail() { echo "$tag FAILED: $*" >&2; exit 1; }

unset WATCHDOG_TICK WATCHDOG_STALL WATCHDOG_KILL_GRACE
echo "$tag running a step that prints once and is silent for 150 s under the default watchdog"
set +e
"$ci_dir/watchdog.sh" --name seeded-silent-step -- bash -c 'echo "seeded step started"; sleep 150; echo "seeded step finished (must never be printed)"' 2>&1 \
  | while IFS= read -r line; do printf '%s %s\n' "$(date -u +%H:%M:%S)" "$line"; done | tee "$work/out.txt"
rc="${PIPESTATUS[0]}"
set -e

[ "$rc" -eq 124 ] || fail "expected exit 124, got $rc"
grep -F "STALLED" "$work/out.txt" >/dev/null || fail "no STALLED line"
if grep -F "must never be printed" "$work/out.txt"; then fail "the silent step ran to the end"; fi
beats="$(grep -c 'state=' "$work/out.txt" || true)"
[ "$beats" -ge 3 ] || fail "expected at least 3 heartbeat lines (30/60/90 s), got $beats"
# heartbeat spacing: consecutive heartbeats at most 31 s apart
prev=""
while IFS= read -r stamp; do
  secs=$((10#${stamp:0:2} * 3600 + 10#${stamp:3:2} * 60 + 10#${stamp:6:2}))
  if [ -n "$prev" ] && [ $((secs - prev)) -gt 31 ]; then fail "heartbeat gap of $((secs - prev)) s"; fi
  prev="$secs"
done < <(grep 'state=' "$work/out.txt" | cut -d' ' -f1)
echo "$tag OK: $beats heartbeats, STALLED reported, step aborted, exit $rc"
