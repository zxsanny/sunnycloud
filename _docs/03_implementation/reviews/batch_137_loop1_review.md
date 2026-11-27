# Code Review Report

**Batch**: 137, AZ-3879 (ST-09-1, ST-09-2, ST-09-3, AT-09-1 after the split) | **Date**: 2026-11-27 | **Verdict**: PASS_WITH_WARNINGS after one auto-fix round (no Critical or High; three Medium spec gaps fixed)

Evidence was gathered by a read-only agent that was given the code and the specs, not this session's reading, and was
checked here against the code before each verdict below.

## Findings

| # | Severity | Category | File:Line | Title | Outcome |
|---|----------|----------|-----------|-------|---------|
| 1 | Medium | Spec-Gap | `test_st_09_1_secret.py` | The status-body check could not fail: `worker_status_report` returns three integers; the free-text `worker_id` and the jobs' failure texts were not scanned. | Fixed: new probe `worker_status_rows` (every row, worker id included); `job_rows` (failure texts) and FD's record (every message to the decoder) join the scanned text; the case asserts there is a status row to scan. |
| 2 | Medium | Spec-Gap | `test_st_09_3_swap_race.py` | Nothing proved the supervisor read while the names were swapped: all exchanges could end before its open and every job succeed. | Fixed (already found on the stack): both outcomes must occur. Run: 45 `succeeded`, 55 `dead`/`output_rejected` of 100. The `exchanged ?` detail no longer raises on a missing match. |
| 3 | Medium | Spec-Gap | `test_st_09_2_no_data_path.py` | Mounts were checked against a deny-list; a blob bind mount at a neutral path would pass. | Fixed: the mount list must equal the two socket folders, the two scratch folders and the token. |
| 4 | Low | Bug | `test_st_09_3_swap_race.py` | The last render's `swap_end` follows the job's close, so the record could be read before it. | Fixed: the record is read once every render has its `swap_end` (bounded wait). |
| 5 | Low | Bug | same | Jobs were matched to photos by creation order, and a dead job's posted preview was never read back. | Fixed: every derived file of the 100 photos that the routes serve must be the WebP; no mapping. |
| 6 | Low | Spec-Gap | `test_st_09_1_secret.py` | Only one of IT-09-13's frame kinds is replayed. | Accepted, documented: every frame kind ends in the decoder client's one protocol-violation path, and that client never holds the token. |
| 7 | Low | Spec-Gap | same | FD's record (what the supervisor sent its decoder) was not scanned; the docstring said "every stack container". | Fixed (with 1); docstring names the services the log list covers. |
| 8 | Low | Spec-Gap | `test_st_09_3_swap_race.py` | No `/proc/self` variant; scratch emptiness after the race not checked. | Fixed: every other render's symlink goes through `/proc/self/root/...`; the general scratch must empty. |
| 9 | Low | Spec-Gap | `test_at_09_1_policy_acceptance.py` | The track-hash equality would pass on two files with no track. | Fixed: the tracks must be exactly video and sound, as in IT-09-4. |
| 10 | Low | Maintainability | worker suite | Duplicated helpers: `closed`, `OUTPUT_ROUTE`, an inline terminal count, a hand-written chunk form, a job wait `jobkit` already has. | Fixed: `h2cases.closed_rows` and `OUTPUT_ROUTE` (IT-09-2/4 use them too), `terminal`, `chunks`, `Media.wait_jobs_closed`. |
| 11 | Low | Maintainability | `fake-decoder/src/serve.rs` | A poisoned lock silently skipped the swap; the doc said "before the next request is read". | Fixed: the value is taken from a poisoned lock (`PoisonError::into_inner`); "answered". |
| 12 | Low | Maintainability | `stackkit.py` | `container_facts` docstring listed four keys of ten. | Fixed. |
| 13 | Low | Spec-Gap | `test_ctl_actions.py` | No unit case showed a retired token counted and never returned. | Fixed: new case. |
| 14 | Low | Scope | `.gitignore` | The `_docs/_autodev_state.md` line removed, unrelated to AZ-3879. | Accepted: the file is tracked and the autodev status check flags the line (worktree-isolation invariant 4); named in the commit. |

## Checked and found fine

- `db_login`: "refused" is reported only after the server was reached (libpq's "no password supplied" follows the
  server's password request; the other two texts are server messages); DNS or TCP failures read as `unreachable`, a
  login without a password as `connected`, so the case fails rather than passing falsely. The service is allow-listed,
  the roles are constants, psql gets no secret, the command has a timeout and stdin from `/dev/null`.
- `secret_scan`: counts only; the retired-token file is in the per-run secrets folder (deleted at teardown, mounted by
  no container); the old token is appended before the current file is overwritten.
- FD `swap`: the rename-exchange never creates or removes an entry; `clear_out` before the second render and the job
  tree's removal after the last stop the loop with ENOENT before the next plant; the supervisor pins the output and
  reopens it through `/proc/self/fd`, so the posted bytes are always the regular file's.
- ST-09-1 case B: `worker_api` turns failing only on a 403, not on a refused connection while the API restarts.
- No changed file over 500 lines (`ctl_actions.py` 498).
