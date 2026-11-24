# Containment: non-dumpable subreaper, kill-all descendants, hostile-class exit, seccomp profile

**Task**: AZ-3855_decoder_containment_kill_all
**Name**: Decoder process containment
**Description**: Make the runner non-dumpable and a child subreaper, kill every descendant and remove `work/` after every job on every exit path, exit after any hostile-class outcome, and ship the seccomp profile that denies `ptrace` and `process_vm_*`
**Complexity**: 5 points
**Dependencies**: AZ-3853_decoder_runner_socket_loop, AZ-3854_decoder_process_guard
**Component**: 10_decoder
**Tracker**: AZ-3855
**Epic**: AZ-3634 (E-19)
**Ref**: E-19/c07a

## Problem

The decoder is assumed compromised (ADR-009, R01). A child that calls `setsid` or double-forks escapes a process-group kill and could read the next job's decrypted input. A peer process could attach to the runner or read its memory. Docker restarts a container on exit, not on a failed health check, so the runner itself must end after any outcome that suggests compromise. The seccomp rule that denies `ptrace` and `process_vm_*` is required by 10 §3 but no other task owns the profile.

## Outcome

- No descendant of the runner survives a job; the next request is accepted only after kill-all and `work/` removal.
- Hostile-class outcomes end the runner after the typed reply; clean `Corrupt` and `Unsupported` do not.
- `ptrace` and cross-process memory reads fail with `EPERM` in the real container.

## Scope

### Included
- At start: `PR_SET_DUMPABLE = 0` and `PR_SET_CHILD_SUBREAPER`.
- After every job on every exit path (success, typed failure, guard kill, `cancel`, connection loss): walk the process table for every process whose ancestry reaches the runner, SIGKILL, reap, repeat until none is left; then remove `work/`; only then send the reply.
- Hostile-class outcomes: a child ended by a signal, `Killed{wall|memory|stall}`, `LimitExceeded`, a descendant found alive at job end (G-10-2, resolved), a peer protocol violation; after the typed reply the runner exits.
- The decoder seccomp profile (deny `ptrace` and `process_vm_readv/writev` with `EPERM`, never by removing the call) as a deployable file for the decoder services, with `no-new-privileges`.

### Excluded
- Safe-open of `in/source`, `job_dir` resolution and the self-abort watchdog (E-19/c07b); container restart itself (Compose `restart: unless-stopped`, checked in E-19/c15); the Compose service definitions that reference the profile (dev E-00/c11, test E-31/c05, release bundle).

## Acceptance Criteria

**AC-1: Escaped descendants are killed before the reply (IT-10-14, NFT-SEC-45)**
Given a job whose child calls `setsid`, double-forks and then ignores SIGTERM
When the job ends
Then the reply arrives only after no process descended from the runner exists, and the runner's process list shows only itself within 1 s of the reply

**AC-2: Next job's input stays unreadable (IT-10-14)**
Given the leftover-seeking child of AC-1 looping on the next job's `in/source` and writing `LEAK` on success
When job B is sent right after job A's reply
Then the leak file is absent or without `LEAK`, and B succeeds

**AC-3: `work/` removed on every path (AC-71, IT-10-16, IT-10-19)**
Given jobs ending in success, `Corrupt`, `Killed{wall}`, `cancel` and connection loss
When each ends
Then `work/` of that job no longer exists

**AC-4: Hostile-class outcomes end the runner (IT-10-16, G-10-2)**
Given a child killed by SIGSEGV, a `Killed{memory}`, a `LimitExceeded`, and a descendant found and killed at job end
When each job ends
Then the typed reply is sent first and the runner exits within 5 s

**AC-5: Clean failures keep serving (IT-10-16)**
Given a truncated JPEG giving `Corrupt` and an SVG named `.png` giving `Unsupported` (G-10-5)
When each job ends
Then the same runner process serves the next job

**AC-6: No attach, no memory read (IT-10-15, NFT-SEC-45 step 3)**
Given a child running as the decoder user in the container with the seccomp profile
When it calls `ptrace(PTRACE_ATTACH, runner)`, `ptrace(PTRACE_TRACEME)`, `process_vm_readv(runner)`, and opens the runner's `mem` and `environ`
Then each returns `EPERM` or `EACCES` (never `ENOSYS`), and the runner's status shows `Seccomp: 2` and `NoNewPrivs: 1`

## Non-Functional Requirements

**Security**
- Surviving descendants after a job = 0 exact; bytes of the next job readable = 0 (NFT-SEC-45 pass criteria).

**Reliability**
- The kill-all walk terminates: it repeats until a full pass finds no descendant.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Kill-all walk with a test child that `setsid`s and double-forks | No descendant left; reply after the walk |
| AC-3 | Exit-path table | `work/` removed on each path |
| AC-4, AC-5 | Outcome classification table | Hostile-class vs clean as specified |

## Blackbox Tests

Runner-level cases need a hostile child on demand, so they use the `test-stubs` variant built by E-19/c15 and run there in the production container; the unit cases above run with test children on a Linux CI host.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | Stub `setsid`/double-fork child, job B normal | IT-10-14 | Dead before B; no leak; B succeeds | NFT-SEC-45, AC-25 |
| AC-3, AC-4, AC-5 | Stub and clean failures | IT-10-16 | Exit on hostile; stay up on clean; `work/` gone | AC-25, AC-71 |
| AC-6 | Stub child calling `ptrace`/`process_vm_readv` | IT-10-15 | `EPERM`/`EACCES`; `Seccomp: 2` | NFT-SEC-45, NFT-SEC-29 |

## Constraints

- A shared-kernel hardening, not a VM boundary; no gVisor in v1.0 (canonical-names).
- The profile denies, it does not remove, the calls (`ENOSYS` would mean the filter is absent).

## Risks & Mitigation

**Risk 1: The walk races a fork loop**
- *Risk*: a fork bomb creates children faster than the walk kills them.
- *Mitigation*: `pids_limit` on the container and the repeat-until-empty rule; a descendant found at job end makes the outcome hostile-class, so the runner exits anyway.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| No task owned the seccomp profile; it is added here. The dev, test and release Compose files (E-00/c11, E-31/c05, release bundle) must reference it, otherwise IT-10-15 fails | E-00, E-31 and release owners | open | Medium |
| Split from E-19/c07 (5 points planned): seven mechanisms plus the unowned seccomp profile exceed 5 points; safe-open and the watchdog moved to E-19/c07b | Parent (Step 4 ref map) | open | Low |
