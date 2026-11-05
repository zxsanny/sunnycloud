# Scrubber, ScrubJanitor and race-free findings

**Task**: AZ-3745_scrubber
**Name**: Scrubber
**Description**: Scheduled and on-demand re-verification of committed blobs that terminates under live traffic, stays under its rate cap, pauses under load, and never reports a blob that is being uploaded or removed
**Complexity**: 5 points
**Dependencies**: AZ-3725_master_keyring, AZ-3728_blob_reader, AZ-3730_gc_sweeper, AZ-3784_sc_audit, AZ-3786_sc_observability
**Component**: 04 blobstore
**Tracker**: AZ-3745
**Epic**: AZ-3627 (E-10)
**Ref**: E-10/c12

## Problem

Corruption at rest must be found and reported by a scheduled scrub that does not slow user transfers (AC-24, AC-7). A naive scrub never finishes while new blobs keep arriving, and it can mistake a half-written upload (unwritten extents read as zeros) or a blob just removed by GC for corruption (AC-80, R13).

## Outcome

- `Scrubber::start_run(mode, rate)` with mode `Full` or `Due` and rate defaulting to `scrub_max_rate` returns `ScrubRun{started_at}`; a run covers only committed blobs with `created_at <= started_at` and `last_scrubbed_at` empty or older than the threshold (`started_at` for `Full`, `started_at - scrub_interval` for `Due`).
- `run_step` picks the oldest batch by `last_scrubbed_at` (empty first), reads ciphertext under the rate cap, verifies every chunk tag, the final-flag chunk and the file length; on a mismatch it re-reads the row in a new short transaction and reports only if the blob is still `committed`: sets `unavailable_at` and writes one `scrub_mismatch` audit event. It updates `last_scrubbed_at` and returns `StepResult{checked, mismatched, missing, done}`.
- In the API process a step pauses while the activity gauge (in-flight block writes and open downloads) is above `scrub_pause_active_streams` (8); a CLI run has no gauge and is throttled only by its rate.
- `finish_run` writes one `scrub_summary` audit event (integer counts, duration) and sets the scrub metrics; `status` returns the oldest `last_scrubbed_at`, the unscrubbed count, the latest summary and the count of blobs with `unavailable_at`.
- `ScrubJanitor::run_cycle(now)` runs a `Due` scrub at the default rate until `done` or `scrub_cycle_budget` (30 min), then `finish_run`.

## Scope

### Included
- The four `Scrubber` operations, the activity gauge counting in the block-write and download paths, `ScrubJanitor`, scrub metrics, INFO cycle logs, WARN when the scrub stays paused.

### Excluded
- `scrub run` and `scrub status` commands and their exit codes (E-22/c12), scheduling (E-22/c06), health exposure (E-10/c13).

## Acceptance Criteria

**AC-1: A run terminates under live traffic**
Given 300 committed 1 MiB blobs and a writer that commits a new blob every 20 ms
When a run starts and `run_step` is called until done
Then `done` becomes true within a bounded number of steps, only blobs created at or before `started_at` are checked, and the new blobs are left for the next run

**AC-2: Real damage is reported**
Given 3 of those blobs with a flipped bit and 2 with the file deleted
When the run finishes
Then `mismatched = 3` and `missing = 2`, each of the 5 has `unavailable_at` and exactly one `scrub_mismatch` event, `finish_run` writes one `scrub_summary` with integer counts and duration, and `status` shows the oldest `last_scrubbed_at`, the unscrubbed count and 5 unavailable blobs

**AC-3: Rate cap and Due mode**
Given `rate = 10 MiB/s`, and blobs scrubbed within `scrub_interval`
When a `Full` run and then a `Due` run execute
Then the measured read rate stays at or below 11 MiB/s, and the `Due` run skips the recently scrubbed blobs

**AC-4: Pause under load**
Given the activity gauge above `scrub_pause_active_streams` in the API process
When `run_step` is called
Then no ciphertext is read while the gauge is above the threshold, and the step resumes when it drops

**AC-5: Scheduled cycle loses nothing**
Given a scheduled cycle interrupted before `done`
When the next cycle runs
Then it recomputes the set from `last_scrubbed_at` (no scrub table), keeps the progress recorded before the interruption, and still covers every remaining eligible blob

**AC-80: Scrub never flags in-flight or collected blobs (component part, R13)**
Given (a) a blob in `uploading` with half its blocks written, (b) a committed blob whose step pauses after reading ciphertext while GC claims it, unlinks it and the state is `gc_pending`, (c) 200 randomised-timing iterations of (b) with real concurrency, and (d) a control blob with real corruption
When the scrub steps run and resume
Then (a) is never selected, (b) and (c) are not reported in any of the 200 iterations (0 audit events, no `unavailable_at`), and (d) is reported exactly once; the same outcome with real uploads, GC and the CLI is checked end to end by E-22/p14

## Non-Functional Requirements

**Performance**
- Write throughput with scrub running at least 95% of baseline, failure below 90%; scrub rate at most 55 MiB/s, failure at 60 MiB/s (PT-04-4, E-10/c14).

**Reliability**
- A run always terminates (`done` defined by `started_at`).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Run-set selection by `started_at` and threshold | Only eligible committed blobs |
| AC-3 | Rate limiter arithmetic | Bytes per second within the cap |
| AC-80 | Re-check after mismatch | Reported only when still `committed` |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 300 blobs, writer every 20 ms | IT-04-11 termination | `done` true; new blobs excluded | |
| AC-2 | 3 flipped, 2 deleted | IT-04-11 findings | 3 and 2; audit and status correct | FT-P-43, NFT-SEC-26 |
| AC-3 | Rate 10 MiB/s; recent scrubs | IT-04-11 rate and Due mode | <= 11 MiB/s; recent blobs skipped | NFT-RES-LIM-05 |
| AC-4 | Gauge above threshold | IT-04-11 pause case | No read while paused | |
| AC-5 | Interrupted cycle | Unit list: "scrub run terminates while new blobs arrive" plus resume | Continues from `last_scrubbed_at` | |
| AC-80 | Cases (a)-(d) | IT-04-12 | 0 false positives in 200/200; control once | NFT-RES-39 |

## Constraints

- No scrub table; the run set is recomputed from `last_scrubbed_at`. Selection takes no row lock.
- No transaction across a ciphertext read; the re-check uses a new short transaction. System scope.
- Audit events carry integer counts and ids only.

## Risks & Mitigation

**Risk 1: Scrub competes with transfers (R12)**
- *Risk*: scrub I/O slows uploads and downloads.
- *Mitigation*: rate cap, activity-gauge pause, short cycles; PT-04-4 gate.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| "Scrub paused too long" is logged at WARN, but no duration defines "too long" | Epic owner | open | Low |
| Whether the internal worker fetch (same reader) counts as a "download" for the activity gauge is not stated | Epic owner | open | Low |
