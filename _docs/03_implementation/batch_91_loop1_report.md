# Batch Report

**Batch**: 91
**Tasks**: AZ-3663_pipeline_gc_retention_restore (one worker; parent review, CI step)
**Date**: 2026-11-18

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3663 | Done (flagged concern G-21 GC interval and trigger stays open, Low) | `server/tests/pipeline_gc_retention.rs` (new, whole file `#![cfg(feature = "test-clock")]`), `pipeline_support/gc.rs`; CI: `rust-test` test-clock step and its README row | 7/7, 3 runs | Discovered #1 |

No production code changed. Time moves through the test clock AZ-4334 wired: `POST /test/clock` on the test-clock listener, with the production 7-day retention (AC-1) and 90-day tombstone horizon (AC-4); the other cases use short config and polling.

| AC | Test | Result |
|---|---|---|
| AC-1 scheduler | originals stay through day 6 (rows, blob rows, files all present after 2 finished GC cycles), gone after day 7 plus 1 hour; no error log lines | pass x3 |
| AC-1 gc-run | `admin gc-run` on the same two-original scenario (`retention` 2 s, real time): removed 2, remaining_due 0, budget_spent false, rows and files gone | pass x3 |
| AC-2 | JPEG with `preview` and `thumb` outputs through the worker routes; after the original's collection the derived rows, blob rows and files go with no further clock move | pass x3 |
| AC-3 | `retention` 10 s: `docs/a.txt` returns with placement `original`; `old/b.txt` (folder deleted) lands in `Restored` at the account root; both download intact and are live feed items; `c.txt` (already collected) gets exit 2 `beyond_retention`, feed head and rows unchanged | pass x3 |
| AC-4 | 90 days plus 1 hour: tombstone purged, purge horizon at or above its seq; a poll below it gets 410 `resync_required` / `below_purge_horizon` | pass x3 |
| AC-5 | see below | pass x3 |
| AC-6 | text check of the non-test callers and registrations (`lists.rs`, `serve/start.rs`, 02 `maintenance.rs`, `domain.rs`, `maintenance_commands.rs`, `dispatch.rs`) | pass x3 |

AC-5 arrangement (from outside, no fault hook in a component): the sweeper parks a due file as `<file>.gc` and then removes the rows in a separate transaction. The test holds `FOR UPDATE` on the deleted file's revision row with a superuser connection, then moves the clock past day 7; the GC claims the blob and parks the file, and 02's `DELETE FROM revisions` waits on the lock. The test proves that window (a backend waiting on a lock in that DELETE in `pg_stat_activity`, the `.gc` file present, all three rows still there), then SIGKILLs the process, releases the lock and restarts. The restarted process starts at offset 0, so the test moves its clock 8 days, past the claim lease; the next cycle finds the file already parked and removes the rows and the parked file, with no error line.

## Code Review Verdict: PASS_WITH_WARNINGS

| # | Sev | Category | Location | Finding | Outcome |
|---|---|---|---|---|---|
| 1 | Low | CI | `ci.yml` | The feature-gated file would not run in CI | **Fixed** (parent): added to the `rust-test` test-clock step |
| 2 | Low | Test | AC-1 gc-run, AC-3 | `admin gc-run` and `admin restore` are separate processes whose clock starts at offset 0 and cannot see the time `serve` was moved to; these cases use short `retention` on real time | Kept (Discovered #1) |

## Test Suite

- `--features test-clock`: `pipeline_gc_retention` 7, `test_clock_serve` 2, `api_services` 4, `composition_root` 12. Default build: `pipeline_gc_retention` compiles with 0 tests (gated), `test_clock_serve` 2 (the release-build AC-3 tests). The worker's 3 runs of the 7 were stable (about 12 s each).
- CI-parity (`CARGO_INCREMENTAL=0`, steps run one at a time, line-buffered): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, server clippy `--features test-clock` and `--features vault`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm`, `check-pinning` exit 0. `CI-parity: PASS`.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | The CLI commands cannot be moved by the test clock (a CLI-side offset would be a production change), so AC-1's gc-run case and AC-3 use short retention on real time; AC-1's scheduler case, AC-2, AC-4 and AC-5 use the test clock | AZ-3663 Constraints | Accept; a CLI offset would need its own task | unclear |
| 2 | After collection 02's cleanup deletes the revision row, so a path restore of a collected file can only fail at the time check, never at a reinstate refusal | AZ-3663 AC-3 | Asserted the observable result (exit 2, `beyond_retention`, nothing changed) | clear |
| 3 | There is no metric for how many blobs a scheduler cycle removed, so AC-1's "same counts" compares gc-run's `removed: 2` with the same two-blob scenario the scheduler collects | AZ-3663 AC-1 | Accept | clear |
| 4 | GC interval and trigger (G-21) is still a decompose-time gap | flagged concern | Unchanged | unclear |

## Commit

`[AZ-3663] GC, retention and restore pipeline through the real binary` — Loop: 1

## Next Batch: cumulative review 89-91 (due: after batch 91), then the recomputed ready list
