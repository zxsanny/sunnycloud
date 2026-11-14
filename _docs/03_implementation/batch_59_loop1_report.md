# Batch Report

**Batch**: 59
**Tasks**: AZ-3713_tree_move_vs_edit (done), AZ-3736_transfer_resume_abort (done), AZ-3843_worker_auth_jobs_facade (done), AZ-3924_vault_unlock_failure_report (done)
**Date**: 2026-11-14

Four parallel workers, one per component (02, 03 with an additive 04 port, 08, 20). The parent made the new `BlobCatalog::get_uploading_plan` a required method (the worker had given it a default body answering `Unexpected`, a placeholder) and implemented it in the `contracts` test double, wrote the remaining document changes, reviewed, fixed two review findings, ran the gate and committed. AZ-3819 (account view and deletion request) was held: it has open owner questions — no port supplies the display name and linked identities for `AccountView`, and the contract changes it needs (CC-1 `lock_deleting`, CC-2 `request_as_operator`) do not exist yet.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3713_tree_move_vs_edit | Done | IT-02-14 race suite: 500 iterations of `update_node(F->G)` against `publish(Existing{f, base=r1})` on separate connections, 400 random-skew at a barrier and 50 per forced order (both calls queue on the account advisory lock held by a third connection; PostgreSQL wakes waiters FIFO). Every iteration: f keeps its id and parent, F under G, new revision current, 0 conflict copies, usage delta once, both changes read through the real 05 feed. AC-1: lock, then cycle check, then stamp, then only COMMIT and notify. No tree fix needed. About 8 s against the 60 s limit |
| AZ-3736_transfer_resume_abort | Done | `get_upload`, `abort_upload` (one short transaction: guard, `DELETE … RETURNING`, release exactly `reserved_bytes`, remove blob row; file discarded after COMMIT), lazy load of the progress entry from the checkpoint row on first access after a restart (no rehydrate). Measured resend: AC-1 8 MiB (4 cut blocks), AC-2 4 MiB against the 16 MiB bound. New port `BlobCatalog::get_uploading_plan` (account-scoped, `uploading` only) served by 04 |
| AZ-3843_worker_auth_jobs_facade | Done | `WorkerAuth`: SHA-256 digests of `worker_api_token` and `worker_api_token_next` only, constant-time compare against every digest, one `Forbidden`, `worker_auth_failures_total`, WARN with no secret. `WorkerJobsFacade` delegates to the queue and derived store (non-holder `LeaseLost`, no input byte). `record_status` upserts `worker_status` on the API clock. Authenticator line coverage 100% |
| AZ-3924_vault_unlock_failure_report | Done | `report_unlock_failure`: account-scoped lookup, then one `vault_failure` audit row and log line through `AuditWriter::record` (fail closed), ids and time only; unknown or foreign vault `NotFound` with nothing written. The placeholder test of the old `Unexpected` answer was removed with the placeholder |

## Parent changes

- `contracts::BlobCatalog::get_uploading_plan` required (no default body); the `contracts` test double implements it.
- 03: every progress-table eviction (abort, expiry in `live_session`, vanished row at checkpoint) goes through `evict`, which lowers `upload_sessions_active` only when an entry was removed (review finding); test `an_expired_session_dropped_from_memory_lowers_the_active_gauge`.
- 20: a duplicate unit test removed (review finding).

## Contract and document changes

`upload_service.md` 1.5.0 (worker: get/abort, lazy load, `AccountNotActive` on abort); `blob-registry-gc.md` 2.1.0 (worker); `worker_job_protocol.md` 1.0.2 (worker); part 3 1.4.0 (parent: `get_uploading_plan` row and change log; note that the 1.3.0 change-log row of batch 58 is missing); 08 description and `deployment/observability.md` list `worker_auth_failures_total` (parent); 20 description (worker).

## Code Review Verdict: PASS_WITH_WARNINGS

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `check-no-sql-now`, `check-db-gates` (3 of 3), `check-audit-sql`, `build-wasm` all exit 0; `cargo test --workspace --locked` on PostgreSQL 18: 3379 passed, 0 failed, 21 ignored. After the two review fixes: fmt, workspace clippy and the 03, 20 and `contracts` tests again green (03: 99 integration passed, incl. the new gauge test). `rust-lint.sh` steps 3-4 (roles, migrations, `cargo sqlx prepare --check`) NOT-RUN(env: no `psql` on this machine); the tree has no sqlx query macros and this batch adds no migration, so the offline data cannot have changed.

Warnings (recorded, not fixed):
- 03 lazy load racing an abort: `load` reads the row and the plan without a lock, so an abort committing in between can leave a stale entry; the next checkpoint finds 0 rows, evicts it and answers `UploadNotFound`, but blocks written before that checkpoint are acknowledged on a cancelled session. No data of another session is affected; the orphan file is the sweeper's.
- 03 `put_block` on an unknown upload id now costs one session query (lazy load) instead of a memory miss.
- 03 `get_upload` after a restart opens the blob file through the full lazy load; a missing file answers `Storage` rather than the state.
- 08 `record_status` stores any worker id; rows go only through the reaper after `job_terminal_ttl`.
- 20 `report_unlock_failure` uses `record` (fail closed); owner flags open: best-effort wish, `NotFound` absent from the frozen port row, no rate limit.

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | 03 cannot reopen an uploading blob's writer after a restart without its key | AZ-3736 notes | Additive port `BlobCatalog::get_uploading_plan` (part 3 1.4.0) | clear |
| 2 | Abort now checks the account guard, so it can answer `AccountNotActive` | AZ-3736 | Recorded in `upload_service.md`; owner may confirm | unclear |
| 3 | Lazy load vs abort race (warning above) | AZ-3736 | Recorded; candidate for the reaper/commit task | unclear |
| 4 | Name of the worker auth failure counter not documented | AZ-3843 | `worker_auth_failures_total` in 08 and observability docs; owner to confirm | unclear |
| 5 | A missing header and an empty bearer both reach the port as an empty string | AZ-3843 | Documented on `authenticate`; 11 must pass `""` | clear |
| 6 | `image_age_days` is `u32` in the contract, `integer` in the column | AZ-3843 | Above `i32::MAX` answers `Unexpected`, writes nothing (tested) | clear |
| 7 | How to force an order in the race suite | AZ-3713 | Third connection holds the account lock; waiters queue FIFO | clear |
| 8 | 20 report flags (best effort, port error row, rate limit) | AZ-3924 | Kept open for E-07/E-30/E-00 owners | unclear |
| 9 | `rust-lint.sh` step 3/4 (roles, migrations, sqlx offline data) needs `psql`, not installed here | CI parity | NOT-RUN(env); no sqlx macros in the tree and no migration in this batch | clear |

## Commit

`[AZ-3713] [AZ-3736] [AZ-3843] [AZ-3924] Move race, resume, worker auth, vault report` — Loop: 1

## Next Batch

Recompute the ready tasks.
