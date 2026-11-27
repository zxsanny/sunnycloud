# Batch 133, loop 1: AZ-4387 input fetch on the real API (IT-09-7, IT-09-6 (i))

**Batch**: 133
**Tasks**: AZ-4387_worker_suite_input_fetch
**Date**: 2026-11-27

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-4387_worker_suite_input_fetch | Done | 21 files | h2: IT-09-7 (a), (b), (d), (e) and IT-09-6 (i) pass; (c) NOT-RUN (D2b) | None |

## What was built

- FD records the input it is handed: each job request's record line gains `input` (`bytes`, `mode`, `gid`, `hash`: the
  BLAKE3 per 2 MiB block roll-up) of `in/source`; a source FD cannot read is an `input_unreadable` event
  (`src/worker/fake-decoder/src/input.rs`).
- e2e-ctl, the fault-injector rows of environment.md: `bit-flip` (one bit of the first ciphertext byte of a blob's chunk
  0, by a throwaway container on the blob volume), `job-row-edit` (new row: one bit of a queued job's `content_hash`,
  one fixed statement), `worker-variant` (new row: the worker recreated with `tests/h2-variants/photo-limit-1mib.yml` or
  `small-scratch-general.yml`, or as the stack defines it), the probe `job_rows`, and `inspect logs` with `lines_with`.
- `suites/worker/test_it_09_07_input_fetch.py`: (a) the edited hash fails `HashMismatch`, not dead, FD saw nothing,
  scratch empty between attempts; (b) a 2 MiB photo under the 1 MiB variant: `TooLarge`, dead after one attempt, FD saw
  nothing; (c) NOT-RUN with the D2b waiver; (d) FD's record shows the source with the uploaded length, mode 0440, group
  10003 and the node's `content_hash`; (e) the general scratch on an 8 MiB tmpfs: `input_too_large`, dead, FD saw
  nothing, no derived row.
- `suites/worker/test_it_09_06_i_fetch_5xx.py` (own module, D2a): the flipped blob gives 500 on the real input route on
  every attempt, the job fails `Transient` and is queued again, the second 500 at least the 1 s backoff after the first
  (1.2 s and then 2.1 s in the run).
- `h2cases`: `fd_on_general` (now the start of `run_file`), `CLEAN_PHOTO`, `job_rows`, `wait_for_row`, `input_errors`,
  `gate_out_both_lanes`, `settled_scratch`.

## Code Review Verdict: PASS_WITH_WARNINGS

`reviews/batch_133_loop1_review.md`: two Low (FD hashes the input per job request; variant names in two places).
The cumulative review of batches 130-133 ran with this batch (it was due after batch 132):
`cumulative_review_batches_130-133_loop1_report.md`, PASS_WITH_WARNINGS after one auto-fix round; its fixes to cases of
AZ-4384, AZ-4385 and AZ-4386 and to e2e-ctl's `start` travel in this batch's commit.

## Test Suite

- H2, fresh stack, `selftests suites/worker` (after every fix of this batch and of the cumulative review): 101
  passed, 0 failed, 4 NOT-RUN (the sentinel, IT-09-6 (c), IT-09-7 (c), IT-09-12 (b)); no `compose start` miss logged.
  Earlier runs on the way: the new cases passed on the first run (5 of 5, (c) NOT-RUN); full runs then showed the
  pre-existing start miss (cumulative finding 11), the stub self-test's fixed action count, the first version of the
  start check failing double swaps, and the IT-09-8 race (finding 12); each was fixed and the last run is the one above.
- `cargo nextest run -p sunnycloud-fake-decoder -p sunnycloud-worker`: 263 passed; `-p sunnycloud-server --test
  crate_graph`: 19 passed. e2e-ctl host unit tests: 74 passed.

CI-parity: PASS (`cargo fmt --all --check`; `cargo clippy -p sunnycloud-fake-decoder -p sunnycloud-http-api
--all-targets --locked -- -D warnings`; the nextest runs above; `scripts/ci/check-pinning.sh`).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | The free-space check compares the scratch mount's free space with the kind's input limit (256 MiB photo, 10 GiB video), not the job's input size, and runs before the fetch (`src/worker/src/slot_loop/job.rs:62`). 09 description.md:99 says the supervisor refuses "a job whose input does not fit". The two agree on a mount sized as the description sizes it (it holds the limit); on a smaller mount every job of the kind is refused, also one that would fit. | IT-09-7 (e), 09 description.md:99 | No change: the description sizes each mount to hold the limit, and a refusal names `input_too_large` (G-09-2). The stack case uses an 8 MiB mount, below the limit, so it holds under either reading. | clear |
| 2 | (b)'s "scratch `source` size 0" cannot be observed on the stack: the job tree is removed with the failure. | IT-09-7 (b) | Asserted by `src/worker/tests/api_input.rs:108`; the stack case asserts `TooLarge`, one attempt, no decoder request, empty scratch. | clear |
| 3 | The api logs non-2xx answers only, so "refused before download" in (e) cannot be shown by counting input requests. | IT-09-7 (e) | `input_too_large` comes from the check that runs before the fetch, and FD saw no request. | clear |
| 4 | Every retry of the flipped blob answers 500 again (chunk 0 is refused each time); the job dies when its attempts run out. | IT-09-6 (i) | The case asserts the first two answers and the backoff between them. | clear |
| 5 | 09 tests.md (e) still called G-09-2 open; `tests/spec-gaps.md:34` resolved it (`input_too_large`, permanent). | IT-09-7 (e) | tests.md (e) reworded; the case asserts the resolution. | clear |
| 6 | The test stack's interactive scratch tmpfs is `size=1g` (`tests/compose.test.yml:41`); 09 description.md:100 fixes 512 MiB and withdraws 1g. | 09 description.md:100 | Out of this task's scope (no case depends on it): leftover `_docs/_process_leftovers/2026-11-27_interactive-scratch-size.md`. | clear |
| 7 | `docker compose start` answered 0 three times without starting the worker (old start time, `exited`); the `lanes` teardown of other cases (IT-09-1, IT-09-5, IT-09-11 a) then failed. Not reproduced in 22 repeats. | harness | e2e-ctl checks each start and tries once more, logging the miss (cumulative review finding 11; gotcha `compose-start-can-answer-0-without-starting`). | clear |

## Commit

`[AZ-4387] Worker suite on H2: input fetch; cumulative review 130-133`

## Next Batch: AZ-3878 (worker integration suite: the remaining IT-09 cases)
