# Cumulative Review — batches 58 to 60 (loop 1)

**Date**: 2026-11-14
**Range**: `9ba4de7..b84d087` (batch 58 `3fd6ddd` + `e5aa29b`, batch 59 `4c2b3af`, batch 60 `b84d087`)
**Verdict**: PASS_WITH_WARNINGS

Each batch had its own review; its findings were fixed or recorded as warnings in the batch report. This pass looks across the three batches for drift no single batch review would see. 171 files changed, about 9,600 lines added: shared test support (`sc_testdb::statements`, `sunnycloud-testkit`), 20 vault service (header, unlock-failure report), sync-core WASM vectors, 02 move-vs-edit race suite, 03 transfer (get/abort, lazy load, `stage_small`), 08 media jobs (worker auth, `WorkerJobs`, erase hook, janitors, admin, doctor, health, metrics), and the S12 download spike record.

## Cross-batch checks

| Check | Result |
|---|---|
| Crate graph (ADR-015) | Holds (`crate_graph` and `contracts` listing tests green in both gates). New normal edges are helpers only (20 on `sc_audit`, `sc_clock`, `sc_ids`; `sync-core-wasm` on `sync-core-proto`); component edges are dev-only; `sunnycloud-testkit` is a dev-dependency only |
| Shared contracts changed | Two, both additive and parent-approved: `VaultHeader.root_node_id` (58, part 3 1.3.0 — its change-log row was missing and is added by this review) and `BlobCatalog::get_uploading_plan` (59, part 3 1.4.0, required method; the worker's placeholder default body was removed) |
| Config table | One reader change: `worker_status_interval` `W` -> `AW` (60, API doctor window) |
| Migrations | One, parent-written and checked up, down, up: `cascade_vault_on_root_purge` (58) |
| Lockfile growth | No new external package; one new workspace crate (`sunnycloud-testkit`) |
| System-scope allowlist | Unchanged; new `run_system` call sites only in files already listed (08 `job_repo.rs`) |
| Secrets | Worker bearer held as SHA-256 digests only, never logged (AC-79 test scans log lines); unlock-failure audit carries ids only |
| One canonical path | One statement capture and one `StateGuard` (58); one eviction path for the 03 progress table (59 fix); 08 metrics extended in `JobMetrics`, not duplicated |
| Production code quality | No `unwrap()` added in production sources, no `#[allow]` added |
| File length | No source or test file over 500 lines; close to the cap: `contracts/tests/blobstore.rs` (494), `02 tests/tree/race_move_edit.rs` (483), `03 tests/transfer/start_support.rs` (482), 08 `job_repo.rs` (463, split once in batch 60) |

## Findings

| # | Finding | Next step | Severity |
|---|---|---|---|
| 1 | Test log capture is now copied three times (`sc_audit`, 01, 20 — batch 59 added 20's, 109 lines) and 08's auth-log test builds its own subscriber | A shared log-capture helper next to `sc_testdb::statements` (same shape as AZ-4250) the next time a suite needs one | Medium (maintainability) |
| 2 | Services built in batches 58-60 have no production wiring yet (`PgVaultService` now needs an `AuditWriter`, `WorkerAuth`, `WorkerJobsFacade`, `MediaLifecycle`, `JobReaper`, `DerivedJanitor`, `MediaAdminService`, `MediaHealth`, `stage_small` behind `UploadService`) | AZ-3789 composition root | Low (expected order) |
| 3 | Two check-then-act races recorded in different components: 03 lazy load vs abort (59), 08 derived janitor vs same-content commit (60) | 03: reaper/commit task E-14/c11-c12; 08: E-18 owner | Low |
| 4 | 03 staged and session expiry are decided in three places (`live_session`, `same_row` does not check `expires_at`, the SQL filters) | Reaper task E-14/c11 should own one expiry rule | Low |
| 5 | Owner proposals waiting: `stage_small` above 2 MiB = `LengthMismatch`; `abort_upload` may answer `AccountNotActive`; 08 doctor/health thresholds and tags; `worker_auth_failures_total` name; 20 unlock report fail-closed vs best effort; gate S12 (Safari) | Owner | — |

## Open findings carried forward

| # | Finding | Status |
|---|---|---|
| 1 | OpenAPI parameters without a pattern | Open, route registry follow-up |
| 2 | `contracts::MasterKey` does not zeroize on drop | Open, contracts owner |
| 3 | IT-12-11 needs `sc_db` test migrations | Open (AZ-3793 partly) |
| 4 | `max_vault_header_bytes` default outside `sc_config` | Open, Low |
| 5 | 07a lock-wait test timing-flaky under load; sync-core `hash_guards` 5% share timing under load | Open, Low |
| 6 | Generated Kotlin and Swift bindings print token fields | Open, E-27/E-28 |
| 7 | Android JNI code and UniFFI CI legs never run | Leftover `2026-11-13_az-3968-android-legs.md` |
| 8 | `rust-lint.sh` steps 3-4 cannot run on this workstation (no `psql`); CI runs them | Environment; install `postgresql-client` or accept CI as the check |
