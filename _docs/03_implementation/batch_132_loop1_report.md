# Batch 132, loop 1: AZ-4386 slots, lanes and the lease on H2 (complete); D2b answered

**Batch**: 132
**Tasks**: AZ-4386_worker_suite_slots_lease; the operator's answer to D2b (recorded)
**Date**: 2026-11-26

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-4386_worker_suite_slots_lease | Done | 7 files | h2: IT-09-9 and both IT-09-10 cases pass | 1 sub-check not asserted (below) |

## What was built

- ctl probes (fixed, read-only): `job_lanes` (`<job id>:<lane>` of every job), `running_lease_ms` (the running job's lease
  expiry), `worker_status_seen_ms` (the time of the latest status report).
- `suites/worker/test_it_09_09_slots.py`: 10 clips (20 poster and transcode jobs) and 20 photos queued while both slots
  are gated out, then FD answering after 300 ms on both sockets: the interactive slot serves only image-lane jobs, the
  general slot's first ten jobs hold at least four of each lane, every job is served once, and no slot runs two jobs at
  once.
- `test_it_09_10_lease.py` (case A): heartbeats measured from the lease expiries the server writes (2,051 and 2,016 ms
  in the run), a test clock advance past the lease, `cancel` before FD's answer, no transcode posted by the attempt that
  lost the lease, the same supervisor claiming the requeued job again on the same connection, the worker unchanged.
- `test_it_09_10_lease_refused.py` (case B, own module): after the API drops the worker's token the `worker_api` item of
  `/readyz` turns failing, the latest status report stops moving and a new photo stays queued.
- `h2cases`: `HELD_TRANSCODE`, `wait_for_request`, `readiness_item`.

## Code Review Verdict: PASS_WITH_WARNINGS

One Low: `job_lanes` returns every job of the module, unbounded; fine at the suite's sizes (tens of jobs).

## Test Suite

- Fresh `h2` stack: IT-09-10 A and B passed; IT-09-9 failed once (a test-ordering fault, below) and passed twice after
  the fix on the kept stack. e2e-ctl host unit tests: 58 passed. No Rust change.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | IT-09-10 holds an FD job 8 s; on a photo render that exceeds H2's `photo_job_timeout` (5 s), so the wall watchdog would cancel first. | IT-09-10 | The case holds a transcode (video timeout 60 s) for 12 s. | clear |
| 2 | With the interactive slot gated out on purpose, the overall `/readyz` is 503 throughout; the cases read the `worker_api` item. | IT-09-10 | `h2cases.readiness_item`. | clear |
| 3 | A supervisor restart (SIGKILL) while a slot holds a job makes that job run again after its lease: an FD swap must happen with the supervisor stopped when a case counts jobs per slot. | IT-09-9 | The case stops the supervisor, swaps both lanes, starts it once. | clear |
| 4 | IT-09-9 "a priority-0 stills job queued behind bulk": no job is bulk below `bulk_queue_threshold` (200 queued jobs). | IT-09-9 | Not asserted; 08's claim order covers it (`components/08_media_jobs/tests/media/queue_claim.rs`). | clear |
| 5 | IT-09-10 B "log has no token value". | IT-09-10 | ST-09-1 (AZ-3879). | clear |

## Commit

`[AZ-4386] Worker suite on H2: slots and lanes, the lease, a refused secret`

## Next Batch: AZ-4387 (input fetch: bit-flip, job hash edit, photo limit, small scratch; (c) waived by D2b), then AZ-3878
