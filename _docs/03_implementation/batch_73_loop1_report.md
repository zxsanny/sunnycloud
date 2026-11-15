# Batch Report

**Batch**: 73
**Tasks**: AZ-3791_serve_start_stop
**Date**: 2026-11-15

One task on the server crate, `sc_config`, helper 07 (`Db::close`), 03 and 04; the parent implemented it inline (the files overlap, no parallel wave).

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3791_serve_start_stop | Done | 34 modified, 9 new (`src/server/src/serve/`, `serve_start.rs`, `serve_stop.rs`, `serve_support/`, 03 `shutdown_flush.rs`, 04 `final_sync.rs`) | 10/10 binary tests (`serve_start` 7, `serve_stop` 3) | None open for this task; see Discovered |

AC-1..AC-6 PASS on the real `sunnycloud` binary against PostgreSQL 18 and a real blob volume. `serve` runs configuration, API pool (role check), `check_version`, key check, epoch marker, services, listeners, late lists, scheduler; every refusal exits 2 before any listener (the three ports probed every 100 ms). Shutdown: 11's four steps, scheduler stopped, 03 `PgUploadService::flush_all`, 04 `SyncBarrier::sync_open_groups`, `Db::close` last; exit 0 on SIGTERM, 130 on SIGINT (`Exit::Interrupted`). Owner decisions of 2026-11-15 applied: keys `public_addr`, `session_cookie`, `binding_cookie`; the two shutdown operations added in 03 and 04; `Db::close` added to helper 07 inside this task (the pool-close step had no port).

## Code Review Verdict: PASS_WITH_WARNINGS

Warnings (recorded, none blocks):
- `update_url` has no key; `HttpServer::new` refuses `min_client_version` without it (`server_info.md` open point 2). `server-info` advertises `0.0.0` when no minimum is set.
- Spawned tasks (request handlers, connections, janitor loops) log through no subscriber: the CLI attaches its subscriber to the command future only. Follow-up AZ-4318 (2 points).
- The SPA bundle is not wired to `HttpServer::with_spa`.
- If `HttpServer::serve` itself returns an error, the scheduler is not stopped and the pool is not closed (the process exits 2 at once). Accepted: no data step is skipped that a clean stop would have done after a failed serve.
- No unit tests with recording steps (spec Unit Tests table): order and refusal mapping are checked on the real binary from its log, plus unit tests of the refusal texts and the 0/130 mapping.

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace `cargo clippy --all-targets --locked -D warnings`, `sunnycloud-server` clippy `--features vault`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm` all exit 0; `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`). `CI-parity: PASS`.

## Test Suite

- Total: 3773 tests
- Passed: 3742
- Failed: 0
- Skipped: 31 (ignored, same count as batch 72; the `#[ignore]`d `the_pool_closes_last` now runs)

`cargo test --workspace --locked --no-fail-fast` on PostgreSQL 18 (`sunnycloud-pg18`, `max_connections=400`).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | Spawned tasks of `serve` log nowhere (subscriber attached per future) | AZ-3791 Outcome "one INFO line per step" | AZ-4318 | clear (cause known: `with_subscriber` is not inherited by `tokio::spawn`) |
| 2 | `.epoch` marker absent on a volume that holds blobs: `serve` passes, doctor fails | AZ-3791 flagged concern 2 | `serve` follows its rule (refuse only when the database epoch is below the marker); doctor owns the absent-marker case | clear (spec rule) |
| 3 | Non-zero code per refusal kind: every refusal exits 2 | AZ-3791 flagged concern 3 | IT-12-6 accepts 2 or 3 | clear |
| 4 | SIGTERM grace: docs say 120 s in `deployment_procedures.md`, 25 s in config | AZ-3791 flagged concern 4 | `serve` uses `shutdown_grace` (25 s) | unclear (docs owner to reconcile) |
| 5 | A failed shutdown step (flush, final sync) still lets the later steps run and ends in exit 2 | AZ-3791 Outcome | as built; the pool closes last either way | clear |

## Commit

`[AZ-3791] [AZ-4318] serve start and stop sequence` — Loop: 1

## Next Batch: AZ-4318 spawned task logs (2 points), then the remaining `todo/` tasks in dependency order
