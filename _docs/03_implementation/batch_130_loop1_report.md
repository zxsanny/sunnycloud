# Batch 130, loop 1: AZ-4384 worker suite on H2, start-up gate, decisions and reconnect (complete)

**Batch**: 130
**Tasks**: AZ-4384_worker_suite_gate_decisions (re-estimated from 3 to 5 points: a 09 product fix and two ctl actions);
the cumulative review of batches 127-129 ran first (`cumulative_review_batches_127-129_loop1_report.md`)
**Date**: 2026-11-26
**Commits**: 4f54286 (review fixes and AZ-4384 part 1, the idle connection check), this batch's commit (the cases)

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-4384_worker_suite_gate_decisions | Done | about 30 files | worker 216/216, ctl host 57/57, consumer unit 42/42, h2 worker suite 14/14 | 2 (below: case times, backoff intervals) |

## What was built

- **09 product fix** (commit 4f54286): `DecoderConn::check_idle` peeks at the socket without blocking before each slot
  round (`slot_loop/mod.rs` `idle_checked`); an end of stream or any byte while idle closes the connection, and the round
  reconnects and gates (`hello`, `self_check`) before it claims. Before, an idle connection whose runner exited stayed
  "ready", claims went on, and the first job on it failed with `DecoderUnreachable`. Proving test
  `src/worker/tests/slot_loop_reconnect.rs` (fails without the fix); `tests/decoder_idle.rs` for the check itself.
- **ctl**: `scratch-fill` (new action, row "Scratch leftovers" in environment.md's fault injectors): leftover job trees
  and a stray file in a lane's scratch volume, owned as a supervisor that died leaves them; `inspect {"container":
  "worker"}`: env names, mounted secrets, mounts and media tools in the container file system (`docker export`);
  `db-probe worker_status_latest`: `decoder_ok` of the newest report (a restarted supervisor reports under a new id).
- **consumer**: `jobkit.py` (the `Media` helper moved out of the media suite's conftest, so both suites use it);
  `fdkit` gets `FAILED_SELF_CHECK`, `JOB_REQUESTS`, `job_requests`; stackkit `scratch_fill`, `container_facts`.
- **Worker suite** `tests/consumer/suites/worker/` (H2 only; its conftest skips it under other profiles):
  - `test_it_09_01_startup_gate.py`: the sweep of both scratch roots at start; with every item ok both slots ready
    (`/readyz` 200, `worker_selfcheck_ok` 1) and a job taken; with one failed item `/readyz` 503, the gauges 0, the latest
    status report `decoder_ok = false`, and no job claimed over 3 s (30 polls); the worker container has no database
    variable, only the `worker_api_token` secret, no blob mount and no media tool.
  - `test_it_09_03_05_decisions.py`: poster offsets 900 ms (9 s clip) and 1,000 ms (60 s), compliant and portrait clips
    skipped with no transcode call, an index at the end remuxed (`plan: "remux"`), 10.5 Mbit/s transcoded with an
    enum-only plan (`encoder cpu`, `faststart`, no tone map); a non-compliant decoder output (IT-09-5) makes exactly one
    transcode call, no transcode output, the job dead after one attempt.
  - `test_it_09_08_obsolete.py`: a photo job whose file was deleted and purged (clock past retention) while the supervisor
    was gated out is closed as obsolete with no decoder call, one attempt, empty scratch, `jobs_total{result=obsolete}` 1.
  - `test_it_09_15_reconnect.py`: RD killed while idle is noticed (503 within 10 s), gated again (200) and counted as one
    restart; with FD's idle outage the new connection's first requests are `hello`, `self_check`, then the job.

## Code Review Verdict: PASS_WITH_WARNINGS

`reviews/batch_130_loop1_review.md`: two Low (Maintainability) open; one Low fixed during the batch (shared FD
constants).

## Test Suite

- `cargo test -p sunnycloud-worker`: 216 passed; `-p sunnycloud-fake-decoder`: 44 passed; `slot_loop_reconnect` and
  `decoder_idle` 20 of 20 repeated runs
- e2e-ctl host unit tests: 57 passed; consumer unit tests: 42 passed
- `tests/run.sh --profile h2` with `selftests suites/worker` (fresh stack): 69 passed, 0 failed, 1 skipped (the NOT-RUN
  sentinel); the 14 worker cases all pass; after the last constant move, again fresh: 70 passed, 1 skipped.
- `tests/run.sh --profile core` with `selftests suites/media suites/operator_cli` (the worker's product change): 71 passed, 0 failed, 6 skipped (the NOT-RUN sentinel, the three AZ-3661 legs, PT-12-1/2)
- CI-parity: `cargo fmt --check` of the changed Rust, clippy of worker and fake-decoder (`-D warnings`), shellcheck of
  `run.sh`; the workspace lint and script tests ran green for 4f54286 in this session.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | The supervisor did not notice an idle decoder connection that ended (found in batch 129). | IT-09-15, AZ-4384 AC-1 | Fixed in 09 (commit 4f54286), with a proving test; the stack case now passes. | clear |
| 2 | `worker_status` keeps one row per worker id, and a restarted supervisor reports under a new id, so the harness probe `worker_status` (all rows) does not show what the latest report says. | IT-09-1 | The suite reads the newest report (`worker_status_latest`). The product already handles stale rows: `doctor`'s worker check reads only rows within `worker_status_interval` times the missed-report count (`components/08_media_jobs/src/worker_health.rs:39`), and old rows are purged (`maintenance_repo.rs:36`). | clear |
| 3 | Measured case times include harness work (a golden reset and sign-in per module, FD swaps, supervisor restarts): 4 s to 90 s, over several of 09 tests.md's "Max execution time" values (15 s to 60 s). The product-side waits inside the cases are short (the IT-09-15 RD case takes 4 s). | AZ-4384 Constraints | The bound is not asserted per case; the times are in `e2e-results/report.csv`. | clear |
| 4 | IT-09-15's "reconnect attempts back off (intervals non-decreasing)": a refused connect leaves no trace FD can record, and the supervisor logs only the first refusal of an outage. | IT-09-15 | Not asserted at the stack; the worker's backoff is covered by its crate tests (`Backoff`, 1 s doubling to 30 s). | clear |
| 5 | IT-09-1 "0 claim requests reach the API (API access log)": the api's access log is not read; the case shows no claim through the job state (never running, still queued) and no job request in either FD record. | IT-09-1 | Equivalent black-box evidence; recorded. | clear |

## Commit

`[AZ-4384] Worker suite on H2: start-up gate, decisions, obsolete, reconnect`

## Next Batch: AZ-4385 (faults and slots on H2), then AZ-3878 (job flows with RD, kills at each step, shutdown)
