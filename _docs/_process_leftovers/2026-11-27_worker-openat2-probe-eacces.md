---
kind: process
timestamp: 2026-11-27T13:25Z
---

# Worker: the `openat2` probe fails with EACCES, so the supervisor always resolves output names on the fallback

**Blocked action**: none blocked; a product defect seen during AZ-3879 (batch 137), outside its scope (test code only).

**What happens**: at start the supervisor probes `openat2(CWD, ".", O_PATH|O_DIRECTORY..., RESOLVE_BENEATH |
RESOLVE_NO_SYMLINKS)` (`src/worker/src/scratch/resolve.rs:25-42`). In the worker image the working directory is
`/home/nonroot` (distroless `nonroot` base, `deploy/docker/worker.Dockerfile:48`, user `10002:10002` at line 58), which
uid 10002 may not open: the probe fails with `Permission denied (os error 13)` and the worker logs "openat2 unavailable,
resolving output names component by component" at every start (H2 stack, Docker 29.1.3, kernel 7.0). `openat2` itself
is available. The component walk refuses symlinks too (`O_NOFOLLOW` per component), so the output check stays safe:
ST-09-3 and IT-09-12 pass on it. But the path `09_worker_supervisor/description.md:53` names (`openat2` with
`RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS`) never runs in a container.

**Fix to make**: probe a directory every uid may open (`/`), or treat only `ENOSYS`/`EPERM` (seccomp) as "unavailable"
and any other error as a probe fault; a unit case that the probe answers "available" when the working directory is
unreadable. Then an H2 run shows no such warning.

**Replay**: a backlog task of epic E-20 (09 Worker Supervisor), about 1 point; delete this note when it is filed.
