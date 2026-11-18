# Batch Report

**Batch**: 92
**Tasks**: AZ-3667_pipeline_scrub (one worker; parent review, fix and CI step); cumulative-review 89-91 fixes
**Date**: 2026-11-18

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3667 | Done | `server/tests/pipeline_scrub.rs` (new), `pipeline_support/scrub.rs` (new); production fix `server/src/scrub_commands.rs`, `key_commands.rs` (`refusal` is `pub(crate)`) | 7/7, 5 runs | Production bug found and fixed; Discovered #1 |
| Cumulative 89-91 | Fixed | `epoch_check.rs`, `migrate_interrupted.rs`, `test_clock_serve.rs`, 08 `obsolete.rs`, `pipeline_tree_conflicts.rs`, `scripts/ci/{install-tools.sh,README.md}`; item 5 helper merge (below) | see Test Suite | none open |

**Production bug the new tests found.** `sunnycloud scrub run` never loaded the master key ring, so on any volume with a committed blob it exited 2 `internal` (KeyUnavailable) with `checked 0`. The existing `scrub_commands` tests use a scripted `Scrubber` and could not see it. Fix: `scrub_commands::run` calls `MasterKeyring::load_and_verify` before `start_run`, as `serve` and `key-check` do; a refusal exits 2 with the key operator message and makes no `start_run` call. New unit test `a_run_checks_the_key_ring_first_and_a_refusal_exits_2_with_no_start_call`; `scrub_commands` and `scrub_sigint` register a scripted key ring. Owner decision "fix now or ticket" was still open at resume; the recommended fix was applied (one call, proven by the 4 failing tests going green) and is reported here.

| AC | Test | Result |
|---|---|---|
| AC-1 clean runs | scheduled cycle and `scrub run --full` find nothing; `scrub status` clean | pass |
| AC-2 corruption | flipped byte + removed file: exit 2, two findings, metric, degraded health (admin health route), `scrub_mismatch` audit, clean download failure; scheduled variant raises metrics and degrades health | pass |
| AC-3 one summary | one `scrub_summary` per scheduled cycle and per CLI run, integer counts | pass |
| AC-4 yields to transfers | scheduled scrub reads nothing while downloads hold the gauge, finishes after | pass |
| AC-5 callers | every F15 operation has a non-test caller | pass |
| AC-80 | full scrubs beside uploads and GC report only the seeded corruptions | pass |

Item 5 helper merge: one `production_text` / `production_source` in `pipeline_support/wiring.rs` (used by `restore.rs`, `feed.rs`, `pipeline_db_restore.rs`) and one `janitor_cycles(ops, task)` in `serve_support/http.rs` (used by `gc.rs` and `test_clock_serve.rs`).

## Code Review Verdict: PASS_WITH_WARNINGS

| # | Sev | Category | Location | Finding | Outcome |
|---|---|---|---|---|---|
| 1 | High | Bug | `scrub_commands.rs` | key ring not loaded before the first step | **Fixed** (above) |
| 2 | Low | Spec wording | AZ-3667 AC-2 | `/healthz` always answers ok; degraded shows at `GET /api/v1/admin/health` | Kept; the test asserts the admin route (Discovered #1) |

## Test Suite

- `pipeline_scrub` 7/7 (default build), `scrub_commands` 7, `scrub_sigint` 1, server lib 24, `pipeline_db_restore` 8 (PG18 tool wrapper), `pipeline_tree_conflicts` 8, `migrate_interrupted` 2, `pipeline_change_feed` 7, media-api 30 + 120, and with `--features test-clock`: `pipeline_gc_retention` 7, `test_clock_serve` 2.
- CI-parity: `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, server clippy `--features vault` and `--features test-clock`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm`, `check-pinning` exit 0. `CI-parity: PASS`. (`check-sqlx-prepare` needs an empty database; no query or migration changed.)

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `/healthz` always answers ok; degraded health shows only at `GET /api/v1/admin/health` | AZ-3667 AC-2 | Spec wording to read "health" not "`/healthz`"; test asserts the admin route | clear |
| 2 | The `scrub_commands` suite used a fake Scrubber only, so wiring gaps in the real CLI were invisible until a pipeline test ran | AZ-3798 | Pipeline suite covers it; no change | clear |

## Commit

`[AZ-3667] Scrub pipeline through the real binary; scrub run loads the key ring` — Loop: 1

## Next Batch: recompute the ready list from `todo/`
