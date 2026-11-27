# Batch 136, loop 1: AZ-3878 AC-2 on H2 (IT-09-2, IT-09-4 with the real decoders); AZ-3878 complete

**Batch**: 136
**Tasks**: AZ-3878_worker_integration_suite (AC-2; with batches 134 and 135 and the split tasks AZ-4381 to AZ-4387 every
AC is covered)
**Date**: 2026-11-27

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3878_worker_integration_suite | Done | 11 files | h2: IT-09-2 and IT-09-4 (3 cases) pass | None |

## What was built

- `suites/worker/test_it_09_02_04_real_decoder.py`, real decoders: IT-09-2, a photo job: one `GET input` (the API's
  `http_requests_total` for the input route) for both renders, preview stored before thumbnail, the derived sizes of
  the 1440 and 320 renders, `jobs_total{kind=stills,result=ok}` +1, empty scratch, no lease left. IT-09-4: a compliant
  landscape clip and a compliant portrait control close `skipped_compliant` with only the poster's thumbnail posted
  and no transcode; the clip with its index at the end is remuxed: index first, every coded packet kept (per-track
  SHA-256 of the samples, equal to ffmpeg's packet hash), `jobs_total{kind=transcode,result=remuxed}` +1.
- `mediakit.track_sample_hashes` (reads `stsz`, `stsc`, `stco`/`co64`); `gen_media.py` makes
  `clip-compliant-portrait.mp4` (720x1280, index first) and checks it like the other compliant clips.
- e2e-ctl `internal-http` `lines_with` (reads up to 4 MiB, keeps the matching lines; for the api's metrics page) and the
  probe `derived_order`; `h2cases` `api_requests`, `derived_order`, `worker_jobs_total`; `Ctl.internal(lines_with=)`.

## Code Review Verdict: PASS_WITH_WARNINGS

The batch review is the cumulative review of batches 134-136 that is due with this batch:
`cumulative_review_batches_134-136_loop1_report.md`, PASS_WITH_WARNINGS after one auto-fix round (four Medium fixed, two
more found and fixed on the stack). Its fixes to IT-09-14, IT-09-17, IT-09-18, IT-09-8, `mediakit` and e2e-ctl travel in
this commit.

## Test Suite

- H2, fresh stack, `selftests suites/worker` (before the review fixes): 110 passed, 0 failed, 4 NOT-RUN (the sentinel,
  IT-09-6 (c), IT-09-7 (c), IT-09-12 (b)); no `compose start` miss.
- After the review fixes, fresh stack: the ctl stub self-test, IT-09-2, IT-09-4 (3), IT-09-8, IT-09-14 (3), IT-09-17,
  IT-09-18: 10 passed and 3 failed (cumulative findings 16 and 17); after their fixes IT-09-14 and IT-09-17 passed
  (4 of 4), and IT-09-14, IT-09-17, IT-09-18 again (5 of 5).
- e2e-ctl host unit tests: 78 passed. `gen_media.py` builds and checks the new portrait clip.

CI-parity: PASS (no Rust change; Python harness, ctl host tests above).

CI of eef64ba (run 45982144674): only `decoder-suite` red, IT-10-18 `it_10_18_sigstop_does_not_abort_the_runner_because_it_stops_the_watchdog_too`
(`src/decoder/tests/container/it_watchdog.rs:124`: the container restarted after the SIGSTOP); it passed on the three
runs before, and eef64ba changed nothing under `src/decoder`. Read as a timing flake on the runner; this push's run is
the re-check (a second failure is a task for AZ-3865's suite). CI of 4eda9c8: only `mobile`, Maven Central 429
(leftover `2026-11-27_mobile-ci-gradle-cache-evicted.md`).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | The real decoder logs no request, so IT-09-2's "render requests `max_edge 1440` then `320`" is not visible as requests. | IT-09-2 | The derived files' long edges (1440, 320) show the sizes; the order shows as preview stored before thumbnail. | clear |
| 2 | A photo job ends within one heartbeat period, so "heartbeat PUTs appear while the job runs" has nothing to observe with the real decoder. | IT-09-2 | The cadence is IT-09-10's (FD holds the job); this case checks that no lease is left after the job. | clear |
| 3 | The spec's packet hash needs ffprobe; the consumer image has no media tools. | IT-09-4 | `mediakit.track_sample_hashes`, checked equal to `ffmpeg -f hash` on the fixture. | clear |
| 4 | DS-35's portrait control had no fixture. | IT-09-4 | `clip-compliant-portrait.mp4` in `gen_media.py`; the media suite commits a fixed list, so its counts do not change. | clear |

## Commit

`[AZ-3878] Worker suite on H2: real-decoder flows; review 134-136`

## Next Batch: AZ-3879 (worker performance, security and acceptance tests), the next ready item of E-20
