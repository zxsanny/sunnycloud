# Batch 137, loop 1: AZ-3879 split; ST-09-1, ST-09-2, ST-09-3 and AT-09-1 on H2

**Batch**: 137
**Tasks**: AZ-3879_worker_perf_security_acceptance (AC-4, AC-5, AC-6, AC-7 first half; the rest split to AZ-4391 and
AZ-4392)
**Date**: 2026-11-27
**Tracker**: AZ-3879 → In Testing (read back); AZ-4391, AZ-4392 created (To Do)
**Last cumulative review**: batches 134-136 (the next is due after batch 139)

## Split

The stack runs one supervisor pair and the consumer uploads one block of at most 2 MiB. AZ-3879 keeps the cases that run
on H2 as it is; the rest moved, each with its own spec in `todo/` and a row in the dependencies table:

- AZ-4391 (5 points): a second supervisor pair on the stack; PT-09-2 and AT-09-2.
- AZ-4392 (3 points): block upload, DS-25, a supervisor resource sample; PT-09-1 and PT-09-3.

The numeric bounds of PT-09-1..3 count only on REF-SRV/REF-CPU4 (`environment.md` § Where each tier runs); both split
tasks record them `NOT-RUN(HW-REF)` elsewhere, with rows in the REF-SRV leftover.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3879_worker_perf_security_acceptance | Done (kept scope) | 20 files | H2: ST-09-1, ST-09-2 (2), ST-09-3, AT-09-1 pass | None |

## What was built

- FD output `swap` (`src/worker/fake-decoder`): a regular file at the result name and a symlink beside it (`out/.swap`);
  after the answer is sent FD exchanges the two names N times with `renameat2(RENAME_EXCHANGE)` and records `swap_end`
  (`exchanged D of N`, and the error that stopped it early, such as the supervisor clearing `out/`). No entry is ever
  created or removed, so the supervisor's `clear_out` and tree removal end the race cleanly.
- e2e-ctl: the secret scan also counts the run's worker tokens (current, next, and each one a `drop-old` rotation
  retired, kept in ctl's own `worker_api_token_retired`); `inspect {"container"}` adds volumes, networks, capabilities,
  the privileged flag and a Docker socket check; `inspect {"db_login": "worker"}` tries a login for four roles from the
  worker's network namespace (psql in a throwaway database-image container sharing it, empty password); probe
  `worker_status_rows`.
- Cases: `test_st_09_1_secret.py` (a real photo job, a hostile frame, the 403 rotation, then the scan of every service
  log, audit rows, `ps`, `/metrics`, `/readyz`, status rows, job failure texts and FD's record: 0 matches);
  `test_st_09_2_no_data_path.py` (exact mount list, no database variable, only the token secret, no blob volume, no
  Docker socket, `cap_drop ALL`, not privileged, network `backend` only; the login is refused for every role);
  `test_st_09_3_swap_race.py` (100 photo jobs, 10,000 exchanges per render, symlink to the secret directly or through
  `/proc/self/root`); `test_at_09_1_policy_acceptance.py` (user-side policy acceptance).
- `h2cases.closed_rows` and `OUTPUT_ROUTE`, shared with IT-09-2/4.

## Code Review Verdict: PASS_WITH_WARNINGS

`reviews/batch_137_loop1_review.md`: no Critical or High; three Medium spec gaps (a status-body check that could not
fail, a swap race that could be vacuous, a deny-list mount check) and ten Low fixed in one round; two Low accepted.

## Test Suite

- H2, fresh stack, run 1 (before the review fixes): the four new modules and IT-09-12: 10 passed, 1 skipped (IT-09-12
  (b), the known NOT-RUN). ST-09-3 alone on the kept stack: passed; split 45 `succeeded`, 55 `dead` `output_rejected`
  of 100, every post 2xx and the WebP.
- Kept stack, the cases using the changed ctl actions: IT-09-1 (4), the FD stack case, IT-09-10 case B, the operator
  CLI's ST-12-1 secret scan: 7 passed.
- H2, fresh stack, run 2 (after the review fixes): the four new modules, IT-09-2/4 (shared helper) and IT-09-12: 14
  passed, 1 skipped (IT-09-12 (b)); ST-09-3 split 35 `succeeded`, 65 `dead`. ST-09-2 again on the kept stack with the
  exact mount list (a fix made after run 2 started): 2 passed (the first rerun stopped on a BuildKit cache error,
  "parent snapshot does not exist"; the retry built and passed).
- fake-decoder: 17 + 9 + 10 + 16 tests pass (3 unit, 1 binary and 1 parse case new); fmt and clippy clean.
- e2e-ctl host unit tests: 83 passed (5 new).

CI-parity: fake-decoder fmt, clippy `-D warnings` and tests green; no other Rust crate changed; CI runs no Python lint.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | The "48-character marker" is a token the consumer may not hold (`environment.md`: the worker secret is held by ctl only). | ST-09-1 | The stack's own token is 48 hex characters (`tests/run.sh`); ctl searches for it and returns counts only. | clear |
| 2 | After the 403 rotation the refused worker still sends the old token, which the secrets folder no longer holds. | ST-09-1 | ctl keeps retired tokens in its own file and scans them too. | clear |
| 3 | "Posted body" is not visible on the wire from the consumer. | ST-09-3 | The output route's answers by status class (the API's sniffer refuses a non-WebP body) and every served derived file read back. | clear |
| 4 | The supervisor's safe open turns a symlink met at open into `output_rejected`, a permanent failure: about half the runs end `dead`. That is the protected outcome ("the regular file's bytes or nothing"). | ST-09-3, IT-09-12, G-09-3 | Both outcomes are required, which proves the race met the open. | clear |
| 5 | "A startup message with an empty password": libpq sends no password for an empty one. | ST-09-2 | The server's password request (or refusal) is the refusal; any login without a password fails the case. | clear |
| 6 | The supervisor always falls back from `openat2` to the per-component walk: its probe opens the working directory, `/home/nonroot`, which uid 10002 may not open (EACCES). | ST-09-3; 09 `description.md:53` | Both paths refuse a symlink and the case passed on the fallback; the probe fix is product work outside this task: leftover `2026-11-27_worker-openat2-probe-eacces.md`. | clear |

## Commit

`[AZ-3879] Worker suite on H2: secret, data path, swap race, policy`

## Next Batch: AZ-4391 or AZ-4392 (split from AZ-3879), or the next ready item of the queue
