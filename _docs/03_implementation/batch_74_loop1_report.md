# Batch Report

**Batch**: 74
**Tasks**: AZ-4318_spawned_task_logs
**Date**: 2026-11-15

One task, done inline by the parent (server crate only).

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-4318_spawned_task_logs | Done | `src/server/src/run.rs`, new `src/server/tests/serve_logs.rs`, 2 docs | 2/2 new pass | None |

`serve` installs its log subscriber as the process default once (`run.rs::install_for_spawned_tasks`), in addition to the per-future subscriber, so request handlers, connections and janitor loops (all on `tokio::spawn`ed tasks) log. A second install in one process refuses with exit 2. Every other command keeps the per-future subscriber only. AC-1 on the real binary (`serve_logs.rs`): a failing `POST /api/v1/uploads` yields one `unexpected failure answered as internal` JSON line, a failing session-reaper cycle yields `janitor ... cycle failed`; the test fails (no lines in 10 s) without the change. AC-2: unit test that a non-`serve` command installs no default.

## Code Review Verdict: PASS_WITH_WARNINGS

Warning: the spec constraint "no process-global default" was narrowed to "none outside `serve`" and the spec says so. Rejected alternative: wrapping eight production spawn sites in four crates with `with_current_subscriber()` (every future spawn site would have to remember it). A test that runs `serve` in-process twice in one test process cannot exist; `serve` tests use the binary.

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `sunnycloud-server` clippy `--features vault`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm` exit 0; `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`). `CI-parity: PASS`.

## Test Suite

- Total: 3775 tests
- Passed: 3744
- Failed: 0
- Skipped: 31 (ignored, unchanged)

## Discovered during implementation

none

## Commit

`[AZ-4318] Spawned task logs reach the sink under serve` — Loop: 1

## Next Batch: remaining `todo/` tasks in dependency order (AZ-3793 migrate commands next)
