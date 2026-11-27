# Cumulative review, batches 130-133, loop 1

**Scope**: `git diff 565e559..HEAD` plus batch 133, without `_docs` (4f54286, c1b94ca, 656f154, dee1b71, 850709e,
a32fe39, c44b8fe and batch 133; AZ-4384, AZ-4381, AZ-3840, AZ-4385, AZ-4386, AZ-4382, AZ-4387) | **Date**: 2026-11-27 |
**Mode**: cumulative (all seven phases, cross-batch first)
**Verdict**: PASS_WITH_WARNINGS after one auto-fix round (no Critical or High; the three Medium are fixed)

Due after batch 132 (the 127-129 report named it) and run one batch late, after batch 133. Evidence was gathered by a
read-only agent and checked here against the code before each verdict below. Findings 1, 4, 6 and the README part of
8 are in batch 133's own code and were fixed in that batch (`reviews/batch_133_loop1_review.md`).

## Findings

| # | Severity | Category | Where | Finding | Outcome |
|---|----------|----------|-------|---------|---------|
| 1 | Medium | Bug | `suites/worker/h2cases.py` `input_errors`, `test_it_09_06_i_fetch_5xx.py` | The case skipped as many input-route error lines as it had counted, in a log tail ctl caps at 4 KiB; the api log lives as long as its container, so after a full run (IT-11-9 adds about 6 lines) the count stops growing and the wait would time out falsely. Other lines holding the route text would have raised `KeyError`. | Fixed in batch 133: the case marks the time of the newest answer and reads answers after it; only `"msg":"request answered"` lines count. |
| 2 | Medium | Bug | `test_it_09_12_hostile_outputs.py`, `test_it_09_03_05_decisions.py` (IT-09-5) | The hostile-output and IT-09-5 cases asserted only "no new derived row, job not running": FD's outputs there were bytes the api refuses anyway, so a supervisor that followed the symlink or posted the non-compliant transcode would still pass. IT-09-5's `dead >= 1` was also met by the poster job, which died of its unpostable output. | Fixed: IT-09-12 asserts its own job's `output_rejected` (G-09-3); IT-09-5 gives FD postable outputs (a WebP poster, an MP4 `ftyp` transcode) and asserts the transcode job's `OutputInvalid`, dead after one attempt, no derived transcode. |
| 3 | Medium | Bug | `test_it_09_13_hostile_frames.py` | Each case's job fails `Retry` and stays queued; the module-wide `dead` count could change when an earlier case's job died in a later case's window. | Fixed: the case asserts its own job's row: a failure recorded, the job `queued` or `running`. |
| 4 | Low | Bug | `suites/worker/conftest.py` `lanes`, `test_it_09_07_input_fetch.py` `worker_variant` | Both fixtures recorded a swap or a variant only after the ctl call succeeded, so a swap whose worker restart failed was never undone. | Fixed in batch 133: recorded before the call. |
| 5 | Low | Bug | IT-09-5, IT-09-6 (e)-(h), IT-09-8 | `db-probe job_attempts` is the most attempts of any job in the module, so `== 1` broke after any earlier retry and `>= 2` held for any job. | Fixed: each case reads its own job through `job_rows` (now with the job kind) and `new_rows`; (e)-(g) also wait on their own job, not on the module's dead count. |
| 6 | Low | Bug | `suites/worker/conftest.py` `worker_state` | A failed `inspect logs` call would have raised `TypeError` inside the failure message. | Fixed in batch 133. |
| 7 | Low | Maintainability | worker suite | Duplicates: `requests_of` with two meanings (`h2cases` lines, IT-09-3 fields); the gate-out of both lanes written inline in IT-09-8 and IT-09-9; the `media` fixture in two suites' conftests; the terminal-state sum in `h2cases` and `jobkit`; 25 hand-written polling loops. | Fixed: IT-09-3's helper is `fields_of`; IT-09-8 and IT-09-9 call `gate_out_both_lanes` (IT-09-9 now also waits for the 503). Accepted: the two `media` fixtures (two suites, two profiles, six lines), the terminal sum and the polling loops (each loop's exit test differs; one `poll()` helper is noted for AZ-3878). |
| 8 | Low | Spec-Gap (doc) | `tests/README.md`, `fake-decoder/src/lib.rs` | The README said 22 fault-injector rows and left out `scratch-fill`, the three input faults and `inspect` `container` / `lines_with`; FD's record example had no `input` key. | Fixed (README in batch 133, the example too). |
| 9 | Low | Bug | `fake-decoder/src/record.rs` | An `out/` FD could not list was recorded as 0 entries, the value IT-09-6 (a) asserts. | Fixed: still 0 (the in-process callers count on a number), plus an `out_unreadable` event; IT-09-6 (a) asserts none occurred. |
| 10 | Low | Style | `11_http_api/Cargo.toml`, `test_it_09_10_lease_refused.py` | Two comments for one dependency, the first stale; IT-09-10 B imported `h2cases` twice, built its marker by hand and asserted inside Act. | Fixed. |

## Found while running the suite (not in the agent's list)

| # | Severity | Category | Where | Finding | Outcome |
|---|----------|----------|-------|---------|---------|
| 11 | Medium | Bug | e2e-ctl `process-kill` `start`, decoder swap `real` | `docker compose start` answered 0 three times in about 120 restarts and left the worker `exited` with its old start time; the `lanes` teardown then waited 90 s and failed (IT-09-1, IT-09-5, IT-09-11 a), and the next case found no worker container. Not reproduced in 22 repeats; cause not found. | Fixed in the harness: `_start` checks for a new start time, logs a miss and starts once more, and two misses are `start_failed`; a container that started and exited is returned as it is, and one that already runs is not started (a lane swapped to FD twice is swapped back twice: the first version of the check failed those teardowns, seen in one run and fixed before the commit). Gotcha `compose-start-can-answer-0-without-starting`. |
| 12 | Low | Bug | `test_it_09_08_obsolete.py` | The case let the interactive FD pass its gate before the general swap restarted the worker, so the old worker could claim and close the job first; the restart then reset the `jobs_total` sample the case reads (0 instead of 1; seen once, after the start check made the swaps slower). | Fixed: the worker is stopped, both lanes swapped, then started, so only the new worker claims. |

## Checked and found fine

- Action contract: the 12 handlers are in `actions.json`; its 25 entries match the 25 rows of the Fault injectors table;
  the stub self-test's `IMPLEMENTED` set matches the handlers. Every `db-probe` name the suites use exists.
- ctl input checks: ids become canonical UUIDs before they reach SQL or a path; variants and lanes are allow-listed;
  every command is an argv list and the shell scripts read their arguments as `"$1"`.
- Restores: worker variants go back to `default` after the decoders; FD swaps are undone by `lanes`; the clock moves,
  the bit flip and the token rotation each sit in a module of their own, which ends with a golden restore.
- Rust: `check_idle` peeks without blocking and drops no error; no `unwrap` in product code; FD's new `input` facts are
  checked against the roll-up by a unit test.
- File length: no changed file over 500 lines (largest `ctl_actions.py`, 475).

## Tests after the fixes

`cargo nextest run -p sunnycloud-fake-decoder -p sunnycloud-worker`: 263 passed; clippy `-D warnings` clean for
fake-decoder and http-api; `cargo fmt --all --check` clean; e2e-ctl host tests: 74 passed; H2 fresh stack, self-tests
and the whole worker suite: 101 passed, 0 failed, 4 NOT-RUN.

Next cumulative review: after batch 136.
