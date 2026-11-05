# Input safe-open, job_dir resolution and the self-abort watchdog

**Task**: AZ-3856_decoder_safe_open_watchdog
**Name**: Decoder safe-open and watchdog
**Description**: Resolve `job_dir` inside `scratch_root` without symlinks, open `in/source` only as a regular single-link file without blocking, and abort the runner when its main loop stops, with the test-only freeze trigger chosen by E-19/c03
**Complexity**: 3 points
**Dependencies**: AZ-3853_decoder_runner_socket_loop, AZ-3851_decoder_watchdog_hook_ssim_spike
**Component**: 10_decoder
**Tracker**: AZ-3856
**Epic**: AZ-3634 (E-19)
**Ref**: E-19/c07b

## Problem

A compromised supervisor could point `job_dir` outside the scratch tree or plant a FIFO, symlink or hard link as `in/source`, making a media tool read `/etc/passwd` or block forever (AC-25). A hung runner leaves its slot dead because Docker ignores failed health checks (R34), so the runner must abort itself; IT-10-18 is blocked until a test can freeze only the main loop (G-10-4).

## Outcome

- Every job reads only a regular, single-link `in/source` inside its own job directory, or fails fast with a typed error.
- A main loop that stops for `decoder_watchdog` ends the process, idle or busy.
- Test builds can freeze the main loop on demand; release builds cannot.

## Scope

### Included
- `job_dir` resolution: must be inside `decoder_scratch_root`, resolved with no symlink anywhere.
- `in/source`: open with `O_PATH | O_NOFOLLOW`, `fstat` regular file with `st_nlink == 1`, reopen through `/proc/self/fd/N` with `O_NONBLOCK`; anything else is `Corrupt`.
- Watchdog: the main loop bumps a monotonic counter at least every `decoder_alive_interval`; a separate thread calls `abort()` when it has not moved for `decoder_watchdog`.
- The test-only freeze trigger of E-19/c03, compiled only into test builds, plus the check that the release binary lacks it.

### Excluded
- Kill-all and hostile-class exit (E-19/c07a); safe-open of `out/` on the supervisor side (E-20/c04).

## Acceptance Criteria

**AC-1: Hostile `in/source` refused without blocking (IT-10-17)**
Given `in/source` as a FIFO, a symlink to `/etc/passwd`, a hard link (`nlink = 2`), a directory and a character device
When a job is sent for each
Then each answers `Corrupt` within 1 s, the FIFO case does not block, no byte of `/etc/passwd` appears in any output or log, and a valid control job runs after each

**AC-2: `job_dir` outside the scratch tree refused (IT-10-17)**
Given `job_dir` values `../x`, an absolute path outside `decoder_scratch_root`, and a symlink inside it
When a job is sent for each
Then each is refused within 1 s with the typed error the tests accept (`Corrupt`, or `ProtocolViolation` for the path cases) and no file outside the job directory is opened

**AC-3: Self-abort (IT-10-18, R34)**
Given a test build with `decoder_watchdog = 3s` and the freeze trigger
When the main loop is frozen while idle, and in a second run while a job runs
Then the process aborts within 3 s +/- 1 s in both runs

**AC-4: Release build has no trigger**
Given the release decoder binary
When the trigger check of E-19/c03 runs, and again on a build that enables the trigger
Then the first finds no trigger and the second fails

**AC-5: No false abort**
Given `decoder_watchdog = 3s` and a 30 s transcode with `decoder_alive_interval = 500ms`
When the job runs to the end
Then the runner does not abort

## Non-Functional Requirements

**Reliability**
- Abort within `decoder_watchdog` (production 20 s, four intervals) +/- 1 s (E-19 NFR table).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Safe-open matrix on a temp scratch root | `Corrupt` per case; FIFO not blocking |
| AC-2 | Path resolution table | Refused without opening outside files |
| AC-3 | Watchdog with a frozen counter (test build) | Abort on time |
| AC-5 | Watchdog with a moving counter | No abort |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | Runner in the decoder image | IT-10-17 | Typed refusals within 1 s; controls pass | AC-25 |
| AC-3 | Test image with the trigger (E-19/c15) | IT-10-18 | Abort and restart on time | AC-25, R34 |
| AC-4 | Release and trigger-enabled builds in the CI `images` job | Trigger check | Release clean; seeded build fails | |

## Constraints

- `abort()` is used on purpose: the container exits and Compose restarts it; the supervisor reconnects (E-20/c05).

## Risks & Mitigation

**Risk 1: The watchdog fires during a long legitimate decode**
- *Risk*: a CPU-heavy job starves the main loop.
- *Mitigation*: the counter is bumped by the loop that also sends `Alive`; AC-5 proves a long job does not abort.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Whether a `job_dir` outside the scratch tree is `Corrupt` or a hostile-class `ProtocolViolation` is not stated (IT-10-17 accepts either) | E-19 owner | open | Low |
