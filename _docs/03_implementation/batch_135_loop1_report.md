# Batch 135, loop 1: AZ-3878 AC-6 on H2 (IT-09-14, kill at fetch, transcode and post)

**Batch**: 135
**Tasks**: AZ-3878_worker_integration_suite (AC-6; AC-2 still open, the task stays in `todo/`)
**Date**: 2026-11-27

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3878_worker_integration_suite | Partial (AC-6 and AC-7 done; AC-2 open) | 13 files | h2: IT-09-14 a, b, c pass | None |

## What was built

- `suites/worker/test_it_09_14_kill.py`, real decoders: (a) the photo's input fetch parked by the API test image's hold
  (AZ-4381) after 16 KiB, (b) the clip's transcode on decoder-general at half a CPU, killed while its ffmpeg runs,
  (c) the photo's thumbnail post parked after its preview was stored. Each: SIGKILL; for (b) the ffmpeg is gone within
  2 s (cancel on the lost socket); the supervisor starts again with both decoders frozen, and both scratch roots are
  empty then with the job still at attempt 1 (swept before any claim); the decoders thaw, the clock moves past the
  lease, the job ends `succeeded` at attempt 2 ((c): the preview answered "already present"); one derived row per kind,
  no unreferenced derived blob, empty scratch, no ffmpeg.
- e2e-ctl `cpu-limit` (new fault-injector row: a decoder's CPU quota through `docker update --cpus`, the previous quota
  in the answer); `h2cases` `arm_hold`, `hold_status`, `release_hold`, `wait_held`.
- Decision D4 applied without asking (record in `_docs/02_task_plans/az-3878-worker-suite/decisions.md`).

## Code Review Verdict: PASS_WITH_WARNINGS

`reviews/batch_135_loop1_review.md`: one Low.

## Test Suite

- H2: fresh stack, then the kept stack: IT-09-14 a and c passed on the first run; b failed twice while it was built
  (the ffmpeg's arguments carry no job id; at 0.2 CPU every attempt ended `DecoderSilent`), then the three legs passed
  together (56 s). The ctl stub self-test (26 actions) passed. e2e-ctl host unit tests: 76 passed.

CI-parity: PASS (no Rust change; ctl host tests above).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | A decoder at 0.2 CPU misses H2's 1 s silence timeout: its jobs fail `DecoderSilent` and die after 3 attempts. | IT-09-14 (b), IT-09-11 | Half a CPU for the killed attempt only, set back right after the kill (D4). | clear |
| 2 | The transcode's ffmpeg reads `/proc/self/fd/9` and writes `result.mp4` relative to the job folder: its arguments name no job. | IT-09-14 (b) | The killed job is the file's `transcode` row. | clear |
| 3 | "Sweep before the first claim" cannot be read with a 6 s real-time lease: a restarted supervisor may claim again before scratch is listed. | IT-09-14 | The restart happens with both decoders frozen (no gate, no claim), scratch is read then. | clear |
| 4 | AZ-3661's media-suite legs "kill during the fetch" and "during the post" can now use the hold. | AZ-3661 leftover | Replay note added to `_docs/_process_leftovers/2026-11-24_az-3661-not-run-legs.md`. | clear |

## Commit

`[AZ-3878] Worker suite on H2: kill at fetch, transcode and post`

## Next Batch: AZ-3878 AC-2 (IT-09-2, IT-09-4 with the real decoders)
