#!/usr/bin/env bash
# Container-level check of the decoder seccomp profile (AZ-3855 AC-6, IT-10-15, NFT-SEC-45 step 3).
# Runs IMAGE the way Compose runs a decoder (--network none --read-only --cap-drop ALL --security-opt no-new-privileges)
# with deploy/seccomp/decoder.json and a small static probe as the image user: ptrace (attach and traceme) and
# process_vm_readv/writev must answer EPERM (never ENOSYS, which would mean the call was removed, not denied), and
# /proc/self/status must show `Seccomp: 2` and `NoNewPrivs: 1`. A control run with Docker's default profile shows the
# same calls succeed, so the denial is the profile's. The probe is compiled here into a temp dir and mounted read-only:
# it is never part of the image. Opening the runner's /proc/<pid>/mem and environ is the runner's PR_SET_DUMPABLE=0
# (AZ-3855 IT-10-15 on the running runner), not the profile's job, and is not checked here.
# usage: check-decoder-seccomp.sh IMAGE      env: SECCOMP_PROFILE (default deploy/seccomp/decoder.json)
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$ci_dir/../.." && pwd)"
profile="${SECCOMP_PROFILE:-$repo_root/deploy/seccomp/decoder.json}"
tag="[check-decoder-seccomp]"
image="${1:?usage: $0 IMAGE}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

command -v gcc >/dev/null || { echo "$tag gcc is needed to build the static probe" >&2; exit 2; }
[[ -f "$profile" ]] || { echo "$tag profile $profile is missing" >&2; exit 2; }
gcc -static -O2 -Wall -o "$work/probe" "$ci_dir/decoder-seccomp-probe.c"

run_probe() { # extra docker args...: the probe output of a decoder-shaped container
  docker run --rm --network none --read-only --cap-drop ALL --security-opt no-new-privileges \
    --user 10003:10003 --volume "$work/probe:/probe:ro" --entrypoint /probe "$@" "$image"
}

failures=0
fail() { echo "$tag FAIL $1" >&2; failures=$((failures + 1)); }
value() { sed -n "s/^$2=//p" <<<"$1" | head -n1; }

echo "$tag running $image with $profile"
denied="$(run_probe --security-opt "seccomp=$profile")"
echo "$denied"
for call in ptrace_traceme ptrace_attach process_vm_readv process_vm_writev; do
  got="$(value "$denied" "$call")"
  case "$got" in
    EPERM | EACCES) ;;
    ENOSYS) fail "$call answers ENOSYS: the call is removed, the profile must deny it with EPERM" ;;
    *) fail "$call answers '$got', want EPERM" ;;
  esac
done
[[ "$(value "$denied" Seccomp)" == "2" ]] || fail "/proc/self/status shows Seccomp: '$(value "$denied" Seccomp)', want 2 (filter mode)"
[[ "$(value "$denied" NoNewPrivs)" == "1" ]] || fail "/proc/self/status shows NoNewPrivs: '$(value "$denied" NoNewPrivs)', want 1"

echo "$tag control run with Docker's default profile"
control="$(run_probe)"
echo "$control"
if [[ "$(value "$control" ptrace_attach)" == "OK" && "$(value "$control" process_vm_readv)" == "OK" ]]; then
  echo "$tag the same calls succeed without the profile: the denial above is the profile's"
else
  echo "$tag NOTE: the control run is denied too (kernel Yama ptrace_scope or a stricter daemon default), so this host cannot show the profile alone is the cause"
fi

if [[ "$failures" -ne 0 ]]; then
  echo "$tag FAILED: $failures problem(s)" >&2
  exit 1
fi
echo "$tag OK: ptrace and process_vm_* answer EPERM under the profile; Seccomp 2, NoNewPrivs 1"
