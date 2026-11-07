# Batch Report

**Batch**: 20
**Tasks**: AZ-3752_quota_recompute, AZ-3725_master_keyring, AZ-3775_feed_stamp, AZ-3839_media_tables_config
**Date**: 2026-11-07

Four workers on four crates (`07a_quota`, `04_blobstore`, `05_sync_feed`, `08_media_jobs` plus `sc_error`) against one PostgreSQL 18 test container. No worker left its `Owns:` paths. Parent edits: `src/sync-core/src/wire_table.rs` (the seven new `sc_error` codes from AZ-3839 stop that crate from compiling until each code has a row; they went into the "no row in the contract's table" group, see item 12), `src/components/05_sync_feed/src/stamp.rs` renamed to `stamp_repo.rs` (the SQL-text gate accepts SQL only in `*_repo.rs` modules), and two owner-approved lines in `scripts/ci/run-system-allowlist.txt` (below). AZ-3729 deferred (same crate as AZ-3725), AZ-3817 deferred (worker cap).

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3752_quota_recompute | Done | `sunnycloud-quota`: `recompute` in `admin_repo.rs` (system scope, `FOR UPDATE`), `CounterDrift`, `recompute_columns`, `impl QuotaAdmin for PgQuotaAdmin` | 7 new (56 in the crate) | Items 1-4 |
| AZ-3725_master_keyring | Done | `sunnycloud-blobstore`: `MasterKeyring` (`load_and_verify`, `init_key_check`, `wrap_data_key`, `unwrap_data_key`), XChaCha20-Poly1305 wrap with AAD = blob id, `.keycheck` file, key ring and rotation | 31 new (127 in the crate) | Items 5-7 |
| AZ-3775_feed_stamp | Done | `sunnycloud-sync-feed`: `PgFeedStamp` (one data-modifying statement), `touched_nodes`, `EmptyTouchedNodes` | 29 + 1 log test; PT-05-1 built, `#[ignore]` NOT-RUN(HW-REF) | Items 8-10 |
| AZ-3839_media_tables_config | Done | `media-api` (08): `MediaConfig`, 08 errors; migration `20261106160000_create_job_tables` (`jobs`, `derived`, `worker_status`, RLS, grants); 7 new codes in `sc_error` | 39 (media-api) + 62 (sc_config) + 22 (sc_error) | Items 11-14 |

## Code Review Verdict: PASS_WITH_WARNINGS

10 findings, none blocking: the stamp leaves a foreign or missing touched node unstamped while the counter still moves; `.keycheck.tmp` is a fixed temp name (concurrent writers can tear it); `init_key_check` can overwrite a `.keycheck` created meanwhile; `ORDER BY random()` sorts every committed blob for the sample; no CHECK ties `jobs.status` to `finished_at` (a terminal row without it is never purged); the seven new codes have no typed row in `sync-core`; the `QuotaAdmin::recompute` port drops the reserved-column drift; a 400 ms wall-clock quota test failed once under full-workspace load; PT-05-1 p95 missed its target in a debug smoke run (AC-5 stays unclaimed until it runs on REF-SRV); the stamp's touched list is unbounded.

CI-parity: PASS after one fix. In `src/` with `CARGO_INCREMENTAL=0`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` (the first run failed on the `sync-core` exhaustive match, fixed), `cargo test --workspace --locked --no-fail-fast`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`, `cargo deny check`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. All RC 0. The full run had one failure, `ledger_locks::second_reserve_waits_for_a_rollback_and_only_it_is_counted` (400 ms wall-clock assertion); it passed 5 of 5 alone and the whole quota crate passed 56 of 56 on re-run. Not run: `check-migrations.sh` (needs `sqlx-cli`).

## Test Suite

- Total: 1829 (workspace, default features, real PostgreSQL 18)
- Passed: 1824 in the full run, plus the one flaky test passed on re-run (1825)
- Failed: 0 after re-run
- Skipped/ignored: 4 (3 as in earlier batches, plus PT-05-1)

## Owner decision applied

The owner approved two more `run-system-allowlist.txt` lines in this session: `src/components/04_blobstore/src/keyring/keyring_repo.rs` (the empty-install check and the 100-blob key sample read `blobs` of every account) and `src/components/08_media_jobs/tests/media/system_scope_repo.rs` (reads all three job tables in system scope and the claim and count plans as the API role).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `QuotaAdmin::recompute` returns one `Drift` (used); the reserved column is reported only by `PgQuotaAdmin::recompute_columns` | AZ-3752 AC-1 | Contract owner: return both columns through the port | unclear |
| 2 | "One WARN per drift found" vs "one WARN line" when both columns drift | AZ-3752 | One WARN per call that finds drift in either column, ids only | unclear |
| 3 | Account without a quota row: contract says "Errors: none", trait has `NotFound` | AZ-3752 | Returns `NotFound` | unclear |
| 4 | Negative `live_bytes`/`reserved_bytes` with `repair` fails the CHECK as `Unexpected` | AZ-3752 | Validate first if wanted | unclear |
| 5 | `MasterKeyring` does not implement the `contracts::MasterKeyring` trait (no `rewrap_batch`, no placeholders) | AZ-3725 | E-10/c13 adds the impl | clear |
| 6 | `KeyError` messages lack the operator text "data is unreadable without the master key"; `operator_message` appends it | AZ-3725 Outcome | Put the text in the `#[error]` strings of `KeyError` (contracts) | unclear |
| 7 | `init_key_check` candidate is key 1; an existing `.keycheck` is only compared; audit before file write; audit carries no actor | AZ-3725 | CLI owner may want `Actor::Operator` | unclear |
| 8 | `contracts::TouchedNodes` has a public field, so an empty set can be built; `FeedStampError` has no empty variant | AZ-3775 AC-4 | Contracts (E-00/c04): private field, checked constructor, error variant | unclear |
| 9 | A touched id not in the account is skipped silently | AZ-3775 | Check stamped count equals touched count | unclear |
| 10 | PT-05-1: p95 103 ms, p99 177 ms vs 50/100 ms in a debug smoke run, throughput 1561/s ok | AZ-3775 AC-5 | Run on REF-SRV before claiming AC-5 | unclear |
| 11 | Codes for the 413 and 422 refusals: `output_too_large`, `output_type_rejected`, `output_kind_rejected` (error names in snake case) | AZ-3839 flagged concern | E-18/E-02 owners | unclear |
| 12 | The seven new codes have no row in `sync_api.md`; `sync-core` reports them as `Unexpected` | AZ-3839 / sync-core | Sync API owner: decide rows (client retry for `thumbnail_not_ready`) | unclear |
| 13 | `jobs` has the tenant policy with a system branch; autovacuum scale 0.02 and fillfactor 70 are the worker's choice | AZ-3839 flagged concerns | Epic owner | unclear |
| 14 | Claim plan is an ordered Index Scan on `jobs_claim_idx`, not index-only (`FOR UPDATE` needs the heap) | AZ-3839 NFR | Amend `data_model.md` 12.5 | unclear |
| 15 | Docs not updated by workers: `sc_error.md` status table (7 rows, minor version), helper 02 rows, `data_model.md` 9.1/12.3 | AZ-3839 | Doc pass at Step 13 | clear |
| 16 | Items 3, 5, 6 of batch 18 and items 1, 3, 9 of batch 19 stay open | batches 18-19 | See their reports | unclear |

## Commit

`[AZ-3752] [AZ-3725] [AZ-3775] [AZ-3839] quota recompute, master keyring, feed stamp, media tables` — Loop: 1

## Next Batch

Recompute from `_docs/02_tasks/_dependencies_table.md` at the start of batch 21. Likely ready: AZ-3729 blob registry, AZ-3817 admin user management, the media queue tasks that depend on AZ-3839, the feed tasks that depend on AZ-3775, plus the spikes. 297 tasks left.
