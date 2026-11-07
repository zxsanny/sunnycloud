# Batch Report

**Batch**: 22
**Tasks**: AZ-3729_blob_registry_gc_queue, AZ-3821_admin_health_aggregation
**Date**: 2026-11-07

Two workers on two disjoint crates (`04_blobstore`, `07_admin`) against one PostgreSQL 18 test container. Only two implementation tasks were ready: every other ready task is a spike (36 of them, mostly owner decisions on open gaps, which workers must not decide; AZ-3893 is Windows only). No worker left its `Owns:` paths. Parent edit: `07_admin` now logs a panicked health check (`sc_observability` path dependency, `log_error!`); the worker's version dropped the `JoinError` silently, which `coderule.md` forbids.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3729_blob_registry_gc_queue | Done; trait impl of `contracts::BlobRegistry` not possible (item 1) | `sunnycloud-blobstore`: `BlobRegistry` (`registry/`), `BlobCatalog` (`catalog/`), `GcQueue` (`gc_queue/`, implements `contracts::GcQueue`); `Row` cause and fault helpers in `error.rs`; `sc_clock` path dependency | 25 new (`tests/rows/`) | Items 1-5 |
| AZ-3821_admin_health_aggregation | Done; `AdminService` trait impl blocked (item 7) | `sunnycloud-admin`: `AdminServiceImpl::health` (`health.rs`, each contributor a task under a timeout), test doubles, `tokio` features | 10 new | Items 6-9 |

## Code Review Verdict: PASS_WITH_WARNINGS

7 findings, none blocking: a panicked health check dropped its error silently (fixed in this batch); `reinstate_in_tx` stats the file before it locks the row (the sweeper AZ-3730 must lock the row too); `register_in_tx` takes the account the port lacks; `schedule` moves a blob to `gc_pending` from any state; the 2 s `health_check_timeout` fallback is a guess; the 20 ms barrier test failed again under load in a worker run; `HealthReport` has no overall status.

CI-parity: PASS. In `src/` with `CARGO_INCREMENTAL=0`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`, `cargo deny check`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. All RC 0. After the logging fix: fmt, clippy for the admin crate, `cargo check --workspace --locked`, the admin health tests (10 of 10), `crate_graph` (10 of 10) and `check-pinning` re-run green. Not run: `check-migrations.sh` (needs `sqlx-cli`).

## Test Suite

- Total: 1941 (workspace, default features, real PostgreSQL 18)
- Passed: 1936
- Failed: 0
- Skipped/ignored: 5 (3 as in earlier batches, PT-05-1, PT-05-2)

## Owner decisions

None needed. No allowlist line was added: the new code has no `run_system` call. No new external dependency.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `contracts::BlobRegistry::register_in_tx(tx, blob, state)` has no account id, but `blobs.account_id` is NOT NULL and the scope setting name is private to `sc_db`; `BlobRegistry` has four inherent methods with an explicit `account` | AZ-3729 contract | (a) add `account` to the port (touches `contracts/tests/blobstore.rs`, `tx_join.rs`, the contract doc, callers AZ-3738 and AZ-3842) or (b) `DbTx::account()` in `sc_db`; then the impl is four delegating methods | unclear, blocks the trait impl |
| 2 | Sweeper race: `reinstate_in_tx` takes `FOR UPDATE`; the sweeper's remove step and claim (AZ-3730) must also lock the row | AZ-3729 / AZ-3730 | Carry into AZ-3730 | unclear |
| 3 | `reinstate_in_tx` on an `uploading` row is an `Unexpected` failure; on a `committed` row with the file present it is `Reinstated` with no change | AZ-3729 | Contract owner | unclear |
| 4 | `schedule_in_tx` has no state guard; a duplicate schedule keeps the earliest `due_at` and its reason | AZ-3729 AC-4 | Guard `uploading` if wanted | unclear |
| 5 | `mark_committed_in_tx` on an absent row or a row not `uploading` is `Unexpected` (matches AZ-3709's case) | AZ-3729 | Keep | clear |
| 6 | `health_check_timeout` is unset; the code falls back to 2 s (same as `/readyz`) | AZ-3821 / AZ-3816 | Confirm 2 s or add a key-table default | unclear |
| 7 | `AdminService` trait impl not done: trait `health` returns a bare `HealthReport` and `AdminError` has no `Forbidden` (E-17/c01 AC-4); `AdminCallError` stays in use | AZ-3821 | Contracts change, then delete `AdminCallError` | unclear, blocked |
| 8 | `HealthReport` has items only, no overall status; tests compute "not ok" locally | AZ-3821 contract open point 3 | Contract owner | unclear |
| 9 | `check` has no error channel: a panic and a `Down` item the contributor returns itself both show `Down`; the panic cause is logged, not copied into the item | AZ-3821 | Keep | clear |
| 10 | Items open from batches 18-21 stay open (see their reports); the 36 ready spikes await owner decisions | batches 18-21 | | unclear |

## Commit

`[AZ-3729] [AZ-3821] blob registry and GC queue, admin health aggregation` — Loop: 1

## Next Batch

No implementation task is ready beyond AZ-3893 (Windows only). The owner must decide the spikes that unblock the most work (AZ-3773 hold-cap error and horizon rule, AZ-3800 409 vs 412, AZ-3695 limited-link rule, AZ-3826 media gaps, AZ-3814 admin gaps) or settle the contract changes (`register_in_tx` account, `AdminError::Forbidden`, `BlobPlan` size). 291 tasks left.
