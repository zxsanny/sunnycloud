# Batch Report

**Batch**: 25
**Tasks**: AZ-3730_gc_sweeper, AZ-3696_sharing_crate_migration_config, AZ-3827_route_registry_openapi, AZ-3840_job_queue
**Date**: 2026-11-07

Four workers on four disjoint crates (`04_blobstore`, `06_sharing` plus a migration, `11_http_api`, `08_media_jobs`) against one PostgreSQL 18 test container. No worker left its `Owns:` paths. Parent edits: two owner-approved allowlist lines; `06_sharing/tests/sharing/rls.rs` (SQL `now()` replaced by a bound timestamp, which `check-no-sql-now.sh` requires); `04_blobstore/src/gc_sweeper/mod.rs` and `tests/gc_sweeper/lease.rs` (a failed cleanup now puts the parked file back). AZ-3743, AZ-3838, AZ-3869 deferred (crate overlap or cap); AZ-3893 deferred (Windows only).

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3730_gc_sweeper | Done; owner confirmed the rename design | `sunnycloud-blobstore`: `GcSweeper` (`gc_sweeper/`: claim with lease, park by rename to `<id>.gc`, settle under row locks, delete or restore the file) | 17 sweeper + 1 log test | Items 1-4 |
| AZ-3696_sharing_crate_migration_config | Done | `sunnycloud-sharing`: `ShareConfig` (`from_settings`, `validate`, `load_refusal`); migration `20261106190000_create_share_tables` (`share_links`, RLS, grants) | 31 | Items 5-7 |
| AZ-3827_route_registry_openapi | Done except the CI job of AC-4 (leftover) | `sunnycloud-http-api`: route registry (`registry/`), deterministic OpenAPI document, `openapi_diff` binary, SPA fallback, listener placement | 47 new (94 integration + 5 unit) | Items 8-10 |
| AZ-3840_job_queue | Done | `media-api`: `JobQueue` (`job_repo.rs`: enqueue, claim, extend, release, complete_skipped, fail, reap, purge, requeue_dead), `retry.rs` | 24 integration + 4 unit new (67 integration) | Items 11-13 |

## Code Review Verdict: PASS_WITH_WARNINGS

10 findings, none blocking; one fixed in this batch (a failed cleanup left the file parked as `.gc`, so a restore or download of a retained version failed until the next cycle after the lease). Open: retained `gc_pending` blobs have no file during a batch; a stalled sweeper can unpark a file another one holds (orphan file); the `.gc` suffix is unknown to the orphan sweep and scrub; job reap scans the claim index; the OpenAPI document is checked only by a structural validator written for the generator; invented cookie name `__Host-session` and a placeholder `server-info` body; the breaking-change gate is not wired into CI; an unreadable grant key is reported as "missing"; settle matches ids by linear search.

CI-parity: PASS. In `src/` with `CARGO_INCREMENTAL=0`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`, `cargo deny check`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. All RC 0. The full workspace run had one failure, `http-api lifecycle::public_and_internal_listeners_serve_h1_and_h2c_until_the_signal_and_run_returns` (it asserts that connecting to the closed listener's ephemeral port fails; under load average ~10 another process can take the freed port); it passed 5 of 5 on a quiet machine. After the late edits: fmt, workspace clippy, blobstore (217), sharing and media-api (102) and the four scripts re-run green. Not run: `check-migrations.sh` (needs `sqlx-cli`).

## Test Suite

- Total: 2150 (workspace, default features, real PostgreSQL 18)
- Passed: 2141 (2140 in the full run + the lifecycle test on re-run)
- Failed: 0
- Skipped/ignored: 9

## Owner decisions

Approved this session: allowlist lines `src/components/08_media_jobs/src/job_repo.rs` (claim has no account yet; reap, purge and requeue cross accounts) and `src/components/04_blobstore/src/gc_sweeper/sweep_repo.rs` (the sweeper sees every account's blob and queue rows). Confirmed: the sweeper parks files by rename instead of adding a claim column.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | The spec order (claim, unlink, settle) cannot be made safe against a restore that stats the file first without a claim marker; park by rename to `<id>.gc`, delete after the rows are gone, restore on `Kept` or `Failed` | AZ-3730 | Owner confirmed | clear |
| 2 | AZ-3743 (orphan sweeper), AZ-3745 and `find_missing` must treat `<id>.gc` as the sweeper's; a crash after settle leaves one orphan `.gc` file | AZ-3730 | Carry into those tasks | unclear |
| 3 | INFO summary cannot carry counts (no log slot); the scheduler must pass its shutdown token | AZ-3730 | `sc_observability` field | unclear |
| 4 | A blob in a sweep batch has no file at its own name until settle; downloads of retained `gc_pending` versions fail `BlobMissing` for that window | AZ-3730 | Accept or serve from the parked name | unclear |
| 5 | `share_grant_key` is a required secret, so the loader refuses missing and empty keys before `Settings` exists; `ShareConfig::load_refusal` maps them to a problem naming the key (reason `MissingRequired` for an unreadable file) | AZ-3696 | Make the key optional in `sc_config`, or add an "unreadable" reason | unclear |
| 6 | `share_password_max_concurrency` and `share_argon2_params` have no defaults; carried as `Option` | AZ-3696 flagged concern | Architect | unclear |
| 7 | `validate` refuses `share_default_ttl` above `share_max_ttl` and zero values in every environment (not in the docs); the invalid-token window has no config key; the later lookup task needs allowlist line `src/components/06_sharing/src/lookup_repo.rs` | AZ-3696 | Doc note; ask then | clear |
| 8 | `utoipa` and an OpenAPI validator crate are not dependencies; document built with `serde_json`, checked by a structural validator | AZ-3827 | Owner decides on dependencies | unclear |
| 9 | CI job for `openapi_diff` not wired; no release tag exists; extension names (`x-auth-class` and five more) not yet in `api_v1_surface.md`; leftover `2026-11-07_az-3827-openapi-diff-ci-wiring.md` | AZ-3827 AC-4 | CI owner | unclear |
| 10 | Cookie name `__Host-session` invented for the document; `server-info` answers `200 {}` until AZ-3804; no `ErrorCode` for 411; embedded web bundle is an in-memory `SpaBundle` built by the composition root | AZ-3827 | 01, AZ-3804, AZ-3935 | unclear |
| 11 | The claim uses the configured `job_attempts` and `job_lease` (not the literal 3); an exhausted job behind other due work stays `running` until reached or reaped; `reap` is not index-assisted | AZ-3840 | Status-led index needs a migration | unclear |
| 12 | `job_leases_expired_total` and `jobs_dead_total` move on takeover, claim-dead, fail-dead and reap (worker's interpretation); `explain_claim` is a public `#[doc(hidden)]` function for the plan test | AZ-3840 | Confirm; feature-gate | unclear |
| 13 | `JobQueue` uses `media-shared` types; the `WorkerJobs` and `MediaAdmin` ports use the `contracts` copies and need a field-for-field conversion in the port tasks | AZ-3840 | Port tasks | clear |
| 14 | Items open from batches 18-24 stay open (purger lock strength, Retry-After, contracts change tickets, sc_error rows for 10 port variants, serde on contracts media model) | earlier batches | | unclear |

## Commit

`[AZ-3730] [AZ-3696] [AZ-3827] [AZ-3840] GC sweeper, sharing setup, route registry, job queue` — Loop: 1

## Next Batch

Recompute ready tasks. Likely ready: AZ-3743 orphan sweeper (must learn the `.gc` suffix), AZ-3838 media policy, AZ-3869 worker API client, and tasks unlocked by the registry, the job queue and the sharing setup. 278 tasks left.
