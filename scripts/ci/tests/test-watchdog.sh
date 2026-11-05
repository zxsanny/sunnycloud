#!/usr/bin/env bash
# AC-4 for watchdog.sh, fast form (intervals shortened through WATCHDOG_TICK / WATCHDOG_STALL):
#   1. a command that is silent for longer than the stall limit is reported STALLED, killed with its children, exit 124
#   2. a command that keeps printing finishes with its own exit code and heartbeat lines
#   3. a failing command passes its exit code through
# The real-time form (30 s tick, 120 s stall, 150 s silent command) is test-watchdog-realtime.sh.
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tag="[test-watchdog]"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail() { echo "$tag FAILED: $*" >&2; exit 1; }

echo "$tag case 1: silent command with a child process, stall limit 4 s"
cat >"$work/silent.sh" <<SH
#!/usr/bin/env bash
echo "started"
sleep 300 &
echo \$! >"$work/child.pid"
sleep 150
SH
chmod +x "$work/silent.sh"
set +e
WATCHDOG_TICK=1 WATCHDOG_STALL=4 WATCHDOG_KILL_GRACE=2 "$ci_dir/watchdog.sh" --name silent -- "$work/silent.sh" >"$work/out1.txt" 2>&1
rc=$?
set -e
cat "$work/out1.txt"
[ "$rc" -eq 124 ] || fail "expected exit 124, got $rc"
grep -F "STALLED" "$work/out1.txt" >/dev/null || fail "no STALLED line"
beats="$(grep -c 'state=' "$work/out1.txt" || true)"
[ "$beats" -ge 3 ] || fail "expected at least 3 heartbeat lines, got $beats"
child="$(cat "$work/child.pid")"
if ps -p "$child" >/dev/null; then fail "child process $child survived the abort"; fi
echo "$tag case 1 OK: STALLED, exit 124, $beats heartbeats, child $child killed"

echo "$tag case 2: command that prints every second for 6 s, stall limit 4 s"
set +e
WATCHDOG_TICK=2 WATCHDOG_STALL=4 "$ci_dir/watchdog.sh" -- bash -c 'for i in 1 2 3 4 5 6; do echo "line $i"; sleep 1; done' >"$work/out2.txt" 2>&1
rc=$?
set -e
cat "$work/out2.txt"
[ "$rc" -eq 0 ] || fail "expected exit 0, got $rc"
grep -F "line 6" "$work/out2.txt" >/dev/null || fail "command output was not passed through"
grep -F "state=PROGRESSING" "$work/out2.txt" >/dev/null || fail "no PROGRESSING heartbeat"
if grep -F "STALLED" "$work/out2.txt"; then fail "a printing command was reported STALLED"; fi
echo "$tag case 2 OK"

echo "$tag case 3: exit code passes through"
set +e
WATCHDOG_TICK=1 WATCHDOG_STALL=4 "$ci_dir/watchdog.sh" -- bash -c 'echo failing; exit 7' >"$work/out3.txt" 2>&1
rc=$?
set -e
cat "$work/out3.txt"
[ "$rc" -eq 7 ] || fail "expected exit 7, got $rc"
echo "$tag case 3 OK"

echo "$tag OK"
