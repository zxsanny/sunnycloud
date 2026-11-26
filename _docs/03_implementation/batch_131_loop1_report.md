# Batch 131, loop 1: AZ-4385 decoder faults on H2 (complete); AZ-4385 split; decision D2

**Batch**: 131
**Tasks**: AZ-4385_worker_suite_faults_slots; the split of AZ-4385 into AZ-4385, AZ-4386, AZ-4387; decision D2
**Date**: 2026-11-26

## Split and decision

AZ-4385 was larger than its 5 points and several legs needed harness pieces that do not exist yet. Split (specs in
`todo/`, Jira, `_dependencies_table.md`):

| Ticket | Task | Points |
|---|---|---|
| AZ-4385 | decoder faults: IT-09-6 (a) to (h), IT-09-11, IT-09-12, IT-09-13 | 5 |
| AZ-4386 | slots, lanes and the lease: IT-09-9, IT-09-10 | 3 |
| AZ-4387 | input fetch on the real API: IT-09-7, IT-09-6 (i) | 3 |

Decision D2 (`_docs/02_task_plans/az-4385-worker-faults/decisions.md`), with a second opinion from an independent agent
whose citations were checked here:
- **D2a, applied without asking**: the fetch 5xx of IT-09-6 (i) comes from the real API through the listed fault injector
  "Bit flip at rest" (to implement in AZ-4387).
- **D2b, open** (`_docs/OPERATOR_TODO.md` D2): IT-09-7 (c), "a body that grows past its Content-Length", cannot happen
  with the real API (the contract fixes an exact `Content-Length`; hyper trims a longer body). Recommended: waive the stack
  leg with a pointer to the worker's own test, and reword 09 tests.md (c). Only that leg waits.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-4385_worker_suite_faults_slots | Done | 6 files | h2: 23 passed, 2 NOT-RUN | 2 NOT-RUN legs (below) |

## What was built (AZ-4385)

- `tests/consumer/suites/worker/h2cases.py`: the shared plumbing of the H2 cases: probe answers in the protocol's form,
  outputs that pass the API's sniffer (a 1x1 WebP, a bare MP4 `ftyp` box: protocol traffic, no media outcome), and
  `run_file` (interactive slot gated out, FD on the general lane, one file committed, its jobs waited for). IT-09-3/5
  now use it too.
- `test_it_09_06_retry.py`: (a) a stalled transcode rerun once on the CPU into an empty `out/`, the job derived; (b) a
  second stall dead; (d) a failed remux rerun as a CPU transcode; (e) to (g) `unsupported`, `corrupt`,
  `limit_exceeded` dead at the first attempt with one probe; (h) a socket closed mid-job retried after its backoff on a
  new connection. (c) NOT-RUN.
- `test_it_09_11_watchdog.py`: a silent decoder cancelled about 1 s after the request and gated again before the next
  job; a render that only sends `Alive` cancelled at the photo timeout (5 s on H2); a stalled render rerun once.
- `test_it_09_12_hostile_outputs.py`: a symlink to the worker secret, a FIFO, a sparse file over the cap and a
  directory never become a derived file and do not hold the slot (the job leaves `running` within 3 s); the regular
  WebP control is posted. (b) NOT-RUN.
- `test_it_09_13_hostile_frames.py`: eight hostile frames (64 KiB + 1, an unknown field, width 0 and 4294967295, a
  negative duration, an unknown media kind, invalid UTF-8, a length then a stall): the job fails transiently, the slot
  reconnects with `hello` then `self_check`, the supervisor keeps running (same start time) and grows under 10 MiB.

## Code Review Verdict: PASS_WITH_WARNINGS

`reviews/batch_131_loop1_review.md`: two Low open.

## Test Suite

- `tests/run.sh --profile h2` with the four new case files (fresh stack): 23 passed, 0 failed, 2 skipped (the NOT-RUN
  legs); the IT-09-3/5 cases after their move to `h2cases`, fresh stack: 6 passed.
- Consumer unit tests: 42 passed. No Rust and no ctl change in this batch.
- CI: run 45852426075 (c1b94ca) and 45852755315 (656f154) were still running when this batch closed; read at the next
  bootstrap.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | IT-09-6 (c) needs a plan that names NVENC; the v1.0 stack runs `nvenc = off`, so no plan does. | IT-09-6 (c) | NOT-RUN(NO-SERVICE) with a pointer to `src/worker/tests/slot_loop_nvenc.rs`; the GPU leg is v1.1 (H-GPU). | clear |
| 2 | IT-09-12 (b), a hard link to the input: with `fs.protected_hardlinks = 1` (this host and common distributions) FD, uid 10003, may not link the supervisor's read-only input, so a decoder cannot plant it either. | IT-09-12 (b) | NOT-RUN(NO-SERVICE) with a pointer to `src/worker/src/scratch/hostile_tests.rs:98`. | clear |
| 3 | IT-09-11 sets `photo_job_timeout=2s` for the wall case; H2 runs 5 s for every case. | IT-09-11 | The case asserts the cancel at the configured 5 s (+0.6 s), the doc's bound scaled to the configured value (durations come from config). | clear |
| 4 | IT-09-12 "no PUT outputs carries any byte of the secret": the API's request bodies are not visible to the suite. | IT-09-12 (a) | The case asserts no derived row and the job left `running`; the supervisor's refusal before reading is `hostile_tests.rs:68`. | clear |
| 5 | IT-09-7 (c) cannot happen with the real API; IT-09-6 (i) needs a server-side fault. | AZ-4387 | Decision D2 (above). | unclear (D2b: the operator's waiver) |

## Commit

`[AZ-4385] [AZ-4386] [AZ-4387] Worker suite on H2: decoder faults; split; D2`

## Next Batch: AZ-4386 (slots, lanes, the lease), AZ-4387 (input fetch; leg (c) waits for D2b), AZ-3878
