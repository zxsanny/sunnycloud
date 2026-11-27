# Batch 134, loop 1: AZ-3878 AC-7 on H2 (shutdown, status, test clock); D3 settled by the spec

**Batch**: 134
**Tasks**: AZ-3878_worker_integration_suite (AC-7 of AC-2, AC-6, AC-7; the task stays in `todo/`)
**Date**: 2026-11-27

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3878_worker_integration_suite | Partial (AC-7 done; AC-2 and AC-6 open) | 12 files | h2: IT-09-17, IT-09-18 and the moved IT-09-10 A pass | None |

## What was built

- e2e-ctl: `process-kill` `stop` for the worker (SIGTERM through `compose stop -t 10`, returns the stop's seconds and
  the exit code) and the probe `worker_status_report` (decoder health, image age, time seen of the latest report).
- `suites/worker/test_it_09_17_shutdown.py`: FD on both lanes built 20 days ago; a clip's transcode held 8 s, a photo
  rendering, a second photo queued; after SIGTERM the stop takes the 3 s grace and exits 0, the held job gets `cancel`
  and is back `queued` with attempts 0, the running photo finishes and the queued one is never claimed; before it the
  status report comes every second, healthy, `image_age_days` 20, on the API's clock.
- `suites/worker/test_it_09_18_test_clock.py` (own module: the offset only grows): heartbeats every 2 s; a 1-hour
  advance ends the running job's lease on the API (cancel, claimed again on the same connection) while no supervisor
  timer fires (no decoder kill, `/readyz` 200); a 3-day advance with the supervisor idle moves `image_age_days` from 20
  to 23 within the interval and one poll; the worker log keeps host time.
- `h2cases`: `lease_samples`, `heartbeat_gaps` (moved from IT-09-10), `status_report`, `next_report`.

## Decision D3 (settled by the spec, not asked)

09 tests.md IT-09-18 expected the running job undisturbed ("no `cancel`") across a 1-hour and a 3-day test-clock
advance. Leases are the API's and follow the offset (`09 description.md:57`; contract `worker_job_protocol.md:67`; the
heartbeat's `available_at > now`, `job_repo.rs:90-96`), which IT-09-10 A relies on. The independent second opinion
quoted the same lines. tests.md reworded; the record is in `_docs/02_task_plans/az-3878-worker-suite/decisions.md`.

## Code Review Verdict: PASS_WITH_WARNINGS

`reviews/batch_134_loop1_review.md`: two Low.

## Test Suite

- H2, fresh stack: IT-09-17 passed (54 s). Kept stack: IT-09-18, IT-09-17, IT-09-10 A passed (4 min 54 s).
- e2e-ctl host unit tests: 75 passed.
- CI of the previous push (07ee255, run 45977837087): only `mobile` red, Maven Central 429 on a cold Gradle cache
  (gotcha `mobile-ci-maven-central-429`); this push's run is the re-check.

CI-parity: PASS (no Rust change; Python harness only; ctl host tests above).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | IT-09-18 "no `cancel` across the advances" contradicts the API-owned lease that follows the offset. | IT-09-18, 09 description.md:57 | D3, settled by the spec; tests.md reworded. | clear |
| 2 | IT-09-18's offset requests "at most 1 per s" and the release image's 0 requests are not visible on the stack (no 2xx log; the stack runs the test image). | IT-09-18 | `src/worker/tests/clock_poll.rs`, `clock_poll_release.rs` and the CI release-symbol check; the case's docstring points there. | clear |
| 3 | IT-09-17's "1 s job" renders twice; held at 1 s per render (about 2 s), so the stop finds it running and it still ends inside the 3 s grace. | IT-09-17 | As built; the margin is in the module. | clear |
| 4 | AC-6's decode leg needs ffmpeg running at the kill; the 1-second fixture clip transcodes in well under a second, so a kill "when ffmpeg is visible" races. | AZ-3878 AC-6, IT-09-14 | Next batch: a deterministic hold on the decode (a CPU limit on the decoder for that case, or a longer fixture), chosen there. | clear |

## Commit

`[AZ-3878] Worker suite on H2: shutdown and status, test clock`

## Next Batch: AZ-3878 AC-6 (IT-09-14, kill at fetch, decode and post with the AZ-4381 hold), then AC-2 (IT-09-2, IT-09-4)
