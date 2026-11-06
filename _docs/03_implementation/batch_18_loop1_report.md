# Batch Report

**Batch**: 18
**Tasks**: AZ-3706_tree_skeleton, AZ-3733_transfer_skeleton, AZ-3750_quota_ledger, AZ-3724_blob_file_format
**Date**: 2026-11-06

Four workers on four disjoint crates (`02_tree`, `03_transfer`, `07a_quota`, `04_blobstore`) against one PostgreSQL 18 test container. No worker left its `Owns:` paths. Parent edits: `src/server/tests/crate_graph.rs` (the "testdb outside dev-dependencies" fixture used `sunnycloud-tree`, which now has a real `sc_testdb` dev-dependency, so the fake edge was no longer new; it uses `sc_names` now), the `steps == 1` assertion of the transfer up/down/up test (now `>= 1`, so later migrations do not break it), and the AC-2 numbers of the AZ-3750 spec (125/124 MiB instead of 124/123 MiB, see item 2).

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3706_tree_skeleton | Done | `sunnycloud-tree`: `TreeConfig`, `TreeError` (10 business variants + database), `run_retrying`; migration `20261106130000_create_tree_tables` (`nodes`, `revisions`, indexes, CHECKs, deferred `SET NULL` FK, RLS, DML grants) | 47 (real PostgreSQL 18, including a real deadlock retried once) | Items 3-6 |
| AZ-3733_transfer_skeleton | Done | `sunnycloud-transfer`: `TransferConfig`, `TransferError` (21 variants), `ProgressTable`/`SessionProgress`; migration `20261106140000_create_transfer_tables` (`upload_sessions`, `commit_receipts`, RLS, grants) | 42 (41 + the allocator-counted memory test: 82.6 MB for 100 x 25,600 blocks) | Items 7-9 |
| AZ-3750_quota_ledger | Done | `sunnycloud-quota`: `PgQuotaLedger` (`reserve`, `release_reservation`, `adjust_used`), `ledger_repo.rs`, `LedgerFault::MissingRow` | 17 new (29 in the crate), including 100 concurrent reservations (exactly 20 granted) | Items 1-2 |
| AZ-3724_blob_file_format | Done | `sunnycloud-blobstore`: `ChunkCipher` (hand-built STREAM-BE32 nonce), `BlobLayout`, header, path, `DataKey`, `FormatError` | 48 new, byte-for-byte against the `aead-stream =0.6.0` encryptor | Items 10-12 |

## Code Review Verdict: PASS_WITH_WARNINGS

7 findings, none blocking: deadlock detection by error-text prefix; `reserve(0)` refused on an over-limit account; no index behind the `SET NULL` foreign key on `nodes.current_revision_id`; `max_inflight_blocks_per_account` derived instead of a config key; `zeroize`, `getrandom`, `thiserror` outside the workspace table; tree and transfer error types without payloads or an unexpected variant.

CI-parity: PASS with a local PostgreSQL 18 (throwaway container). In `src/` with `CARGO_INCREMENTAL=0`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast` (one failure, the `crate_graph` fixture above; fixed and `-p sunnycloud-server --test crate_graph` re-run green, 10 of 10), `cargo test -p sc_observability` without `db`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`, `cargo deny check`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. All RC 0. Not run: `check-migrations.sh` (needs `sqlx-cli`; the up/down/up tests run the same migrations through `sc_db::Migrator`).

## Test Suite

- Total: 1690 (workspace, default features, real PostgreSQL 18)
- Passed: 1687
- Failed: 0
- Skipped: 3 (as in batch 17)

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | Missing `quota` row: all three ledger calls return `Unexpected` with `LedgerFault::MissingRow`, ids only | AZ-3750 flagged concern | Keep; the account hook creates the row | clear |
| 2 | AC-2 numbers: limit 1 GiB, used 800, reserved 100 MiB means 124 MiB fits exactly, so `reserve(124 MiB)` cannot be refused. Test uses 125 (refused) then 124 (exact fit). The contract's reserve-refused case has the same numbers | AZ-3750 AC-2, quota-ledger contract | Spec fixed here; fix the contract text in the doc pass | clear |
| 3 | `reserve(0)` on an over-limit account returns `QuotaExceeded` (rule applied literally) | AZ-3750 | Decide whether a zero-byte reserve short-circuits to `Reserved` | unclear |
| 4 | `sc_db` has no deadlock variant; `run_retrying` matches the `SQLSTATE 40P01` text prefix of `DbError::Other` | AZ-3706 AC-6 | Add `DbError::Deadlock` in `sc_db`, drop the string match | clear |
| 5 | `sc_config` has no default for `purge_batch_size`, `tree_maintenance_interval`, `listing_max_limit`; `TreeConfig` supplies 5000 and 30 s itself; `listing_max_limit` stays `Option` (owner picks a value) | AZ-3706 AC-5 | Move the two defaults into `sc_config`'s table | unclear |
| 6 | No index on `nodes (current_revision_id, account_id)` for the `ON DELETE SET NULL` check; `client_meta` size CHECK is 4096 (my choice); purge must delete revisions before children before parents | AZ-3706 | Measure in the purge task; add a partial index if needed | unclear |
| 7 | `max_inflight_blocks_per_account` is derived (sessions x lanes = 32), not a key | AZ-3733 | Add `max_inflight_blocks_per_account: Count = dflt("32")` to `src/shared/config/src/table.rs` and `01_helper_config.md` | clear |
| 8 | "80 MB" read as 80 MiB (measured 78.7 MiB, 82.6 MB decimal); bitmap bit order (LSB first) is not in the data model | AZ-3733 AC-6 | Record both in `data_model.md` | clear |
| 9 | `TransferError` has no `UploadNotFound`/`Unexpected`; 409 vs 412 for `need_blocks` still open | AZ-3733 | Settle in E-14 | unclear |
| 10 | Empty blob has one tag-only final chunk (file length 80) | AZ-3724 AC-4 | Say so in the spec and `description.md` section 4 | clear |
| 11 | AAD is the raw 16 bytes of the blob id; the key-wrap task must use the same. Range end is inclusive, so `end == size` is not satisfiable. Counter maximum is refused for every chunk including the last (blob cap about 256 TiB) | AZ-3724 AC-2, AC-5 | Carry into E-10/c05 | clear |
| 12 | `zeroize`, `getrandom`, `thiserror` declared in the blobstore manifest (`zeroize = "1.8"`, resolves to 1.9.1, already in `Cargo.lock`) | AZ-3724 | Add to `[workspace.dependencies]` and switch to `workspace = true` | clear |
| 13 | System-scope RLS probes for `nodes`, `revisions`, `upload_sessions`, `commit_receipts` need lines in `scripts/ci/run-system-allowlist.txt`. The batch 17 approval covered only the blob and quota files | AZ-3706, AZ-3733 | Owner decision needed before adding any line | unclear |
| 14 | Disk: 98% used. The first full gate run exhausted it and failed with a compile error; deleted the regenerable `src/target/debug/incremental` (20 GB) and ran with `CARGO_INCREMENTAL=0`. `src/target/debug` is still about 125 GB | process | Prune stale `deps` or move `target` to a larger disk | unclear |
| 15 | Carried from earlier batches and still open | batches 2-17 | See their reports | unclear |

## Commit

`[AZ-3706] [AZ-3733] [AZ-3750] [AZ-3724] tree, transfer, quota ledger, blob format` — Loop: 1

## Next Batch

Recompute from `_docs/02_tasks/_dependencies_table.md` at the start of batch 19. Likely ready: AZ-3727 group commit barrier, AZ-3729 blob registry, AZ-3751 quota admin lifecycle, AZ-3708 tree folder mutations chain (needs AZ-3775 feed stamp), AZ-3774 sync feed skeleton, AZ-3696 sharing migration, plus the spikes. 304 tasks left.
