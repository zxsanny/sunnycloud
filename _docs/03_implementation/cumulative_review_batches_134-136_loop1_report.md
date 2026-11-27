# Cumulative review, batches 134-136, loop 1

**Scope**: `git diff 07ee255..HEAD` plus batch 136, without `_docs` (4eda9c8, eef64ba and batch 136; AZ-3878 AC-7, AC-6,
AC-2) | **Date**: 2026-11-27 | **Mode**: cumulative (all seven phases, cross-batch first)
**Verdict**: PASS_WITH_WARNINGS after one auto-fix round (no Critical or High; the four Medium are fixed)

Evidence was gathered by a read-only agent and checked here against the code before each verdict below. The fix round
then met two timing faults of its own on the stack (findings 16 and 17), fixed before the commit.

## Findings

| # | Severity | Category | Where | Finding | Outcome |
|---|----------|----------|-------|---------|---------|
| 1 | Medium | Bug | `test_it_09_18_test_clock.py` | The log-time check could not fail: nothing made the worker write a line after the advances, so the newest line was older than both. | Fixed: the interactive FD restarts after the 3-day advance (the supervisor logs the ended connection) and the newest line must fall between the advance and now. |
| 2 | Medium | Bug | `test_it_09_18_test_clock.py` | "stays 23" read a report that `next_report` returns unchanged after its timeout. | Fixed: the later report must be newer. |
| 3 | Medium | Bug | `test_it_09_14_kill.py` | The decoder pauses sat outside the `try`, and the teardown never unpaused: a failed pause left a decoder frozen for the rest of the run (a golden reset does not touch decoders). | Fixed: pauses inside the `try`; the teardown unpauses any paused decoder. |
| 4 | Medium | Bug | `test_it_09_17_shutdown.py` | Nothing showed the photo still running at SIGTERM: had it finished first, the drain would pass without draining. | Fixed: just before the stop one photo job is `running` and one `queued`. |
| 5 | Low | Bug | e2e-ctl `cpu-limit` | It refused `cpus` 0, the value its own answer reports for "no quota", so restoring an unlimited decoder would fail. | Fixed: 0 is accepted as no quota. |
| 6 | Low | Bug | `test_it_09_14_kill.py` | "Swept at start" did not show that the kill left anything to sweep. | Fixed: the scratch after the kill must hold a job tree. |
| 7 | Low | Bug | `test_it_09_14_kill.py` | "No ffmpeg left" cannot fail for the photo legs (vips, never ffmpeg). | Fixed: no process besides the runner in either decoder. |
| 8 | Low | Style | `test_it_09_14_kill.py` | The `held` checks came after the restart, so a hold that never held showed as `KeyError`. | Fixed: checked right after the wait. |
| 9 | Low | Performance | `test_it_09_02_04_real_decoder.py` `closed` | A predicate that ignored its row ran one or two probes per row of every poll. | Fixed: one plain polling loop. |
| 10 | Low | Bug | same, `fetches`/`posts` | Counted 2xx only: a failed second fetch would still read as "one fetch". | Fixed: every status class. |
| 11 | Low | Maintainability | e2e-ctl `internal-http` `lines_with` | A filtered answer over 4 KiB was cut silently and could shorten a value. | Fixed: refused (413 `filtered_too_large`). |
| 12 | Low | Bug | `mediakit.track_sample_hashes` | Two tracks of one handler overwrote each other; a file with no track gave `{}`; more samples in the chunks than in `stsz` raised `IndexError`. | Fixed: each is an assert; IT-09-4 checks the tracks are exactly video and sound. The digest stays ffmpeg's packet hash (sizes are not hashed, as ffmpeg does not), so it matches `ffmpeg -f hash`. |
| 13 | Low | Maintainability | worker suite | Duplicates: `obsolete_count` beside `worker_jobs_total`; `INPUT_ROUTE` twice; IT-09-14's helpers beside the media suite's crash helpers. | Fixed the first two. Accepted the third (two harnesses, two leases); the replay note of AZ-3661's leftover names sharing them. |
| 14 | Low | Spec-Gap | IT-09-17, IT-09-18, IT-09-2 | IT-09-17 compares `seen_at` with host time at offset 0; IT-09-18 did not check the worker kept its start time (a restart would reset the kill counter) nor wait for the requeued job before the 3-day advance; IT-09-2 repeated an implied check. | Fixed IT-09-18's two and dropped IT-09-2's; IT-09-17 accepted (with offset 0 the API clock is host time, and IT-09-18 covers the offset). |
| 15 | Low | Style | docstrings, README | "a fifth of a CPU" against 0.5; "1 s job" against about 2 s; README without `stop` and `lines_with`. | Fixed. |
| 16 | Medium | Bug | `test_it_09_14_kill.py` | Found on the stack after the fixes: with the extra scratch read the 6 s lease had already run out in real time, the new supervisor claimed attempt 2 at once, and the clock move then ended attempt 2's lease too (attempts 3). | Fixed: the clock moves while the restarted supervisor is gated, after the kill and before any claim. |
| 17 | Low | Bug | `test_it_09_17_shutdown.py` | Found on the stack: the first status report after a start can predate the gate (`decoder_ok` 0; leftover `2026-11-26_worker-status-first-report.md`). | Fixed: the case starts from the first healthy report. |

## Checked and found fine

- No changed file over 500 lines (largest `ctl_actions.py`, 493).
- ctl inputs: `stop` for the worker only; `cpus` a bounded number (bool, text, NaN refused), services allow-listed, the
  value formatted into argv; `lines_with` a non-empty text, used only to filter in Python.
- `actions.json` has 26 entries for the 26 Fault injectors rows; the stub self-test's `IMPLEMENTED` set matches.
- State: the clock offset dies with the api restart of each module's golden reset; the hold's DELETE disarms it; the
  CPU quota is set back right after the kill and again at teardown; a stopped worker is started by the `lanes` teardown.
- The sample-table walk: `hdlr`, `stsz`, `stco`/`co64` and `stsc` layouts, 64-bit and size-0 boxes; the marker `free`
  box sits after the last box, so chunk offsets hold.

## Tests after the fixes

Fresh stack: the ctl stub self-test, IT-09-2, IT-09-4, IT-09-8, IT-09-14, IT-09-17 and IT-09-18; then IT-09-14 and
IT-09-17 again after findings 16 and 17 (4 passed), and IT-09-14, IT-09-17, IT-09-18 once more: 5 passed.
The whole worker suite on a fresh stack before the fix round (run 16): 110 passed, 0 failed, 4 NOT-RUN. e2e-ctl host
tests: 78 passed.

Next cumulative review: after batch 139.
