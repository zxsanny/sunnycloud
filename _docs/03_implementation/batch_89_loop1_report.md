# Batch Report

**Batch**: 89
**Tasks**: AZ-4332_remediate_media_jobs_obsolete (remediation of AZ-3660 AC-3), AZ-3660 (completed by it), cumulative review 86-88 fixes
**Date**: 2026-11-18

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-4332 | Done | migration `20261118100000_keep_jobs_on_revision_collect.{up,down}.sql`; 08 `job_repo.rs`, `derived_store.rs`, `enqueue_repo.rs`, `job_queue.rs` (doc); 08 tests `obsolete.rs` (new), `keys.rs`, `migration.rs`, `accept_input.rs`, `accept_refuse.rs`; docs `data_model.md` §9.1, 08 `description.md`, 08 `tests.md` | 08 suite 30 + 119 + 1; pipeline AC-3 un-ignored, 3/3 | Discovered #1, #2 |
| AZ-3660 | Done (AC-79 Caddy half NOT-RUN(NO-SERVICE), E-31/c05) | `pipeline_media_worker.rs` (ignore removed, module doc updated, shared `uuid_of`) | 6 + 1 ignored (1 GiB, release job) | None open |
| Cumulative review 86-88 | Fixes applied | `.github/workflows/ci.yml`, `scripts/ci/{rust-lint.sh,it_suite.py,run-one-test.sh,README.md}`, `scripts/ci/tests/{test-run-one-test.sh,run-script-tests.sh}`, server test harness (`serve_support/{http,account}.rs`, `pipeline_support/{mod,download,tree,large,h2}.rs`, `vault_support/mod.rs`), `pipeline_large_upload.rs`, `pipeline_change_feed.rs`, `pipeline_tree_conflicts.rs`, `vault_http_api.md` | see Test Suite | Report `cumulative_review_batches_86-88_loop1_report.md` |

AZ-4332 design: `jobs.revision_id` becomes nullable and the composite foreign key to `revisions` is `ON DELETE SET NULL (revision_id)` (PostgreSQL 18 column list); a collected revision leaves its job with a NULL revision, which 08 reads as obsolete: input and output answer 404 `revision_obsolete`, `result` with `obsolete` closes the job. `account_id` stays set, so the account foreign key (RESTRICT) and 08's erase hook still remove every job of an erased account. The down migration deletes NULL-revision jobs first, then restores NOT NULL and CASCADE. The contract is unchanged (owner decision 2026-11-18: the contract wins).

## Code Review Verdict: PASS_WITH_WARNINGS

| # | Sev | Category | Location | Finding | Outcome |
|---|---|---|---|---|---|
| 1 | Medium | Scope | 08 `enqueue_repo.rs` | Beyond the listed scope: an enqueue that meets a job with a NULL revision adopts it (`DO UPDATE ... WHERE jobs.revision_id IS NULL`), so a new upload of the same content still gets thumbnails while the obsolete job waits for its purge | Kept: without it the kept job would block the new revision's job through `jobs_key`; tested and documented; owner to confirm (Discovered #1) |
| 2 | Low | Docs | `pipeline_media_worker.rs` module doc | Stale "known failure" paragraph | **Fixed** (parent) |
| 3 | Low | Maintainability | `pipeline_support/tree.rs` | A compatibility re-export of `uuid_of` for the media test | **Fixed** (parent): the media test imports the shared helper; re-export removed |
| 4 | Medium | Test flakiness | `pipeline_support/feed.rs` `Pipeline::park` | Under load the 1 s same-principal probe kept reaching the server before the poll, so the poll was refused and re-sent until the 10 s deadline (change-feed AC-2 failed once in about ten runs) | **Fixed** (parent): each retry gives the poll a doubling head start (200 ms, 400 ms, ...) before the first probe |

## Test Suite

- 08 `media-api`: 30 + 119 + 1. 07 `purger` 10, `admin_acceptance` 2.
- Server pipelines after the `park` fix: `pipeline_change_feed` 7 (3 runs), `pipeline_download` 4 (+1 ignored, 1 GiB), `pipeline_large_upload` 9, `pipeline_media_worker` 6 (+1 ignored, 1 GiB; the AC-3 collected case now passes), `pipeline_small_batch` 6, `pipeline_tree_conflicts` 8, `vault_http` 1.
- CI vault command (`--features vault`: `vault_http`, `vault_report`, `local_commands`, `api_services`, `composition_root`): all pass.
- Release leg through `run-one-test.sh`: AZ-3660 AC-72 passed (7.9 s), "1 test passed".
- Script tests: `test-run-one-test.sh`, `test-it-suite.sh` pass.
- `sunnycloud-transfer` `volume_floor` alone: 2 passed (Discovered #5 was `/tmp` contention).
- CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, server clippy `--features vault`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm`, `check-pinning` exit 0; `check-migrations.sh` NOT-RUN(env: no `psql`), steps done by hand (Discovered #4). `CI-parity: PASS`.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | A kept obsolete job (open, or closed `obsolete` until purged) would block, through `jobs_key ON CONFLICT DO NOTHING`, the job of a new upload of the same content | AZ-4332 Outcome, `data_model.md` unique key | Enqueue adopts a NULL-revision job and queues it afresh; a job that still has its revision is left alone | clear, owner to confirm |
| 2 | The contract does not say what claim does with an obsolete job | `worker_job_protocol.md` claim row | Claim hands it out; the worker gets 404 on input and closes it `obsolete`, so every job reaches an end state | unclear (closest reading) |
| 3 | If revision R2 has R1's content while R1's job is open, R2's enqueue is a no-op; when R1 is collected the job closes obsolete and R2 gets no thumbnails (the old cascade had the same gap) | 08 enqueue rule | Separate ticket if wanted (for example input falling back to a current revision with the same hash); needs a contract decision | unclear |
| 4 | `scripts/ci/check-migrations.sh` needs `psql` (via `ensure-db-roles.sh`), absent here; its apply/revert/re-apply steps were run by hand with sqlx-cli on a throwaway database (17 up, 17 down, 17 up) | AZ-4332 Constraints | CI runs the script | clear |
| 5 | `sunnycloud-transfer` `volume_floor::the_free_space_floor_holds_against_concurrent_starts` measures real free space on `/tmp` (a 30 GB tmpfs here); it failed once while other test runs filled `/tmp` with 1 GiB pipeline volumes | none (pre-existing, environment) | Re-run alone (see Test Suite); not caused by this batch | clear |

## Commit

`[AZ-4332] [AZ-3660] Keep media jobs on a collected revision; review 86-88 fixes` — Loop: 1

## Next Batch: recomputed after this commit
