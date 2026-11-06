# Batch Report

**Batch**: 17
**Tasks**: AZ-3749_quota_skeleton, AZ-3723_blobstore_skeleton
**Date**: 2026-11-06

Two workers on disjoint crates (`components/07a_quota`, `components/04_blobstore`), each with its own migration and typed config and error conversions, both on the shared `sc_testdb` crate. Parent edits: the `sc_db` migration tests no longer hard-code the migration list (`embedded()` in `src/shared/db/src/tests/accounts_base.rs` reads the versions from `src/migrations/*.up.sql`; `schema_state`, `interrupted`, `health` use it), so a new migration needs no edit there. The blob migration was renamed from `create_blobs_tables` to `create_blob_tables` so it matches the owner glob `src/migrations/*_blob_*.sql` of `module-layout.md`. No allowlist, advisory or dependency pin change.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3749_quota_skeleton | Done | `sunnycloud-quota`: `QuotaConfig` (default limit, 2^53 bound), error enums `QuotaLedgerError` and `QuotaAdminError`; migration `20261106110000_create_quota_table` (table, RLS policy `quota_tenant`, DML grants) | config, errors, migration (up/down/up, grants, constraints), RLS (account scope) | Two error enums instead of the spec's single `QuotaError`; `default_quota` (key) vs `default_quota_bytes` (field); system-scope half of AC-2 added after the batch (follow-up commit) |
| AZ-3723_blobstore_skeleton | Done | `sunnycloud-blobstore`: `BlobstoreConfig` (`from_settings`, `validate`), `BlobFault` and eight conversion functions; migration `20261106120000_create_blob_tables` (`blobs`, `blob_gc`, RLS, grants) | 29 (config 7+, errors, migration, RLS) | System-scope half of AC-3 added after the batch (follow-up commit); migration name differs from the task text; FK, CHECK and key-id choices below |

## Code Review Verdict: PASS_WITH_WARNINGS

Review of the joined tree: 7 findings, none blocking: `quota.limit_bytes` has no CHECK; `blob_gc` FK is `ON DELETE CASCADE`; master key id 1 is assumed to be the file key; all database failures become `BlobError::Io`; system-scope RLS reads of the blob and quota tables were untested (closed by the follow-up); two error reasons are imprecise.

CI-parity: PASS with a local PostgreSQL 18 (throwaway container, roles as in earlier batches). All 15 steps of the gate RC 0: `cargo fmt --all --check`, clippy workspace `-D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo test -p sc_observability` without `db`, the dependency-graph check of worker and decoder, the four wasm32 builds, `cargo deny check`, `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`.

## Test Suite

- Total: 1579 (workspace, default features, real PostgreSQL 18; includes the extra `sc_observability` run without `db`)
- Passed: 1576
- Failed: 0
- Skipped: 3 (as in batch 16)

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | System-scope reads of `blobs`, `blob_gc` and `quota` need `Db::run_system`, allowed only in allowlisted repository files. Owner decision after the batch: add the lines. Done: `src/components/04_blobstore/tests/blobstore/system_scope_repo.rs` and `src/components/07a_quota/tests/rls/system_scope_repo.rs` are on `scripts/ci/run-system-allowlist.txt`, each with one test (4 blobs and 2 queue entries; both quota rows) | AZ-3723 AC-3, AZ-3749 AC-2 | Closed | clear |
| 2 | Quota error shape: the spec names one `QuotaError`; the contract has two traits, so two enums | AZ-3749 | Update the spec and the component description | clear |
| 3 | `blob_gc` foreign key `(blob_id, account_id) -> blobs(id, account_id)` is `ON DELETE CASCADE`; `reason` NOT NULL with a CHECK on four values; `blobs` has CHECKs on `size` (non-negative, set once committed) | AZ-3723, data_model 6.1/6.2 | Record in `data_model.md`; say if `RESTRICT` is wanted | unclear |
| 4 | Master key id 1 is the `master_key_file` key and a ring entry for id 1 is accepted; the keyring task must settle it | AZ-3723 | Decide in the keyring task | unclear |
| 5 | `quota.limit_bytes` has no CHECK in the database (only the application bounds it to 0..2^53) | AZ-3749 | Add `CHECK (limit_bytes IS NULL OR limit_bytes >= 0)` in a follow-up migration or before the first release | unclear |
| 6 | Doc updates: `01_helper_config.md` (`orphan_mode` values, ring and current rules), `data_model.md` 6.1/6.2/8.2, `module-layout.md` and the task text (blob migration name), component `tests.md` lists | both | Apply in the doc pass | clear |
| 7 | Disk: 97% used, 12 GB free after the gate; `src/target` is the cause | process | See batch 16 item 5 | unclear |
| 8 | Carried from earlier batches and still open | batches 2-16 | See their reports | unclear |

## Commit

`[AZ-3749] [AZ-3723] quota and blobstore skeletons` — Loop: 1

## Next Batch

Ready now: AZ-3893 platform fs windows (cannot be compiled here); the skeletons unblock more tasks, to be computed from `_docs/02_tasks/_dependencies_table.md` at the start of batch 18. 308 tasks left.

## Follow-up: system-scope tests (owner decision, no ticket)

The owner approved two allowlist lines. Each crate has one probe file named `system_scope_repo.rs` with one test that reads through `Db::run_system`; the quota probe is a module of `tests/rls.rs` (`#[path]`). `check-db-gates.sh` 3 of 3, clippy and the tests of both crates pass.

Commit: `[AZ-3749] [AZ-3723] system-scope RLS tests and allowlist lines` — Loop: 1
