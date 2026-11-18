# Batch Report

**Batch**: 88
**Tasks**: AZ-3657_pipeline_download, AZ-3658_pipeline_change_feed, AZ-3659_pipeline_tree_conflicts, AZ-3660_pipeline_media_queue_worker_api (two workers, two tasks each in sequence; parent review, decisions and CI job)
**Date**: 2026-11-18

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3657 | Done (AC-4 Caddy NOT-RUN(NO-SERVICE), E-31/c05) | `server/tests/pipeline_download.rs`, `pipeline_support/download.rs` | 5 run in the suite + AC-1 (1 GiB) in the release job; 3 runs each | None open |
| AZ-3658 | Done | `server/tests/pipeline_change_feed.rs`, `pipeline_support/feed.rs`; production: `server/src/root/{base,api}.rs` (`DbMetrics`) | 7/7, 3 runs | None open |
| AZ-3659 | Done | `server/tests/pipeline_tree_conflicts.rs`, `pipeline_support/tree.rs`; spec G-1 flag marked resolved (`spec-gaps.md` G-1, 2026-11-11) | 8/8, 3 runs | None open |
| AZ-3660 | Partly, stays in todo (AC-3 collected case waits for AZ-4332; AC-79 Caddy NOT-RUN(NO-SERVICE)) | `server/tests/pipeline_media_worker.rs`, `pipeline_support/media.rs` | 5 run in the suite + AC-72 (1 GiB) in the release job; AC-3 collected ignored as KNOWN-FAIL(AZ-4332) | Contract vs as-built (Discovered #1) |

Production change: `serve` never registered `sc_observability`'s database collector, so `/metrics` exported no `db_up`, `db_pool_connections`, `db_query_duration_seconds` or `migrations_pending` (AZ-3658 AC-2 needs the pool family). New context item `DbMetrics` in `root/base.rs`, requested by `api_services`; registered once per context.

Owner decisions of 2026-11-18 (asked in this session):
- AZ-3660 AC-3: the contract wins (404 `revision_obsolete`, then close `obsolete`). Remediation task AZ-4332 filed under E-18 (`_docs/02_tasks/todo/AZ-4332_remediate_media_jobs_obsolete.md`, dependency of AZ-3660 in the table). The honest test stays in the tree, ignored as `KNOWN-FAIL(AZ-4332)`; AZ-4332 un-ignores it.
- The two 1 GiB ACs (AZ-3657 AC-1, AZ-3660 AC-72; about 8 min each in a debug build) run in a new release-mode CI job `pipelines-large` (40 min, PostgreSQL 18 service, each test by exact name), apart from the instrumented `rust-test` job (25 min limit).

## Code Review Verdict: PASS_WITH_WARNINGS

| # | Sev | Category | Location | Finding | Outcome |
|---|---|---|---|---|---|
| 1 | High | Spec-Gap | 08 `jobs` foreign key `ON DELETE CASCADE` | A job on a collected revision vanishes; the worker gets 409 `lease_lost` instead of 404 `revision_obsolete` (contract IT-08-12) | Escalated, owner decided: remediation AZ-4332; failing test kept, ignored with the ticket |
| 2 | Medium | Test infrastructure | `server/tests/serve_support/mod.rs` `Ports::free()` | The probe listeners are dropped before `serve` binds, so a parallel test can get the same port from the kernel (seen in an AZ-3660 worker run and in the joint run: "cannot bind the public listener") | **Fixed** (parent): a port is never handed out twice in one test process (a process-wide list; a refused port's listener stays open while the next is taken). A collision with another process remains possible but CI runs one test binary at a time |
| 3 | Medium | Performance (CI) | AZ-3657 AC-1, AZ-3660 AC-72 | ~8 min each in debug; would break the 25-min `rust-test` job | **Fixed**: ignored in the suite, run in `pipelines-large` (release) |
| 4 | Low | Maintainability | `pipeline_support/{download,tree}.rs` | One shared `impl Pipeline` namespace: two `delete_*` helpers were named apart to avoid a clash; `feed.rs` has its own source walker next to `wiring.rs` | Kept; merge in the cumulative pass if it grows |

## Test Suite

- After the port fix, two consecutive runs: `pipeline_download` 4 (+1 ignored: 1 GiB), `pipeline_change_feed` 7, `pipeline_tree_conflicts` 8, `pipeline_media_worker` 5 (+2 ignored: 1 GiB, KNOWN-FAIL(AZ-4332)), `pipeline_small_batch` 6, `pipeline_large_upload` 9, `serve_start` 7, `serve_stop` 3, `vault_http` 1. 0 failures both runs.
- Release legs as the `pipelines-large` job runs them: AZ-3657 AC-1 passed (7.5 s), AZ-3660 AC-72 passed (6.9 s).
- Server suites touched by `DbMetrics`: `api_services`, `composition_root`, `composition_scheduling`, `serve_logs`, `janitor_ops`, `command_config`, `crate_graph`, vault tests in both builds: all pass.
- CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, server clippy `--features vault`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm`, `check-pinning` exit 0. `CI-parity: PASS`.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | Jobs are deleted with their collected revision (cascade), so no `obsolete` close is possible | AZ-3660 AC-3, `worker_job_protocol.md` | AZ-4332 (owner: contract wins) | clear |
| 2 | Not only the DB collector: `ratelimit_collector` and `audit_collector` are registered nowhere in production (`auth_rate_limited_total`, `audit_write_failures_total` never exported) | E-09 `observability.md` | Follow-up task in E-09 | clear |
| 3 | The debug `serve` build streams 1 GiB at about 2.5-2.9 MB/s; in release both 1 GiB tests take 7-8 s | AZ-3657 AC-1 NFR | Release job `pipelines-large`; perf in E-31/c43 | clear |
| 4 | A 404 for a deleted file carries the problem JSON but no file bytes; the test reads "no body bytes" that way | AZ-3657 AC-5 | Confirm the reading | unclear |
| 5 | Below-purge-horizon case arranged by superuser SQL on `account_feeds.purge_horizon` (the 90-day purge is not reachable over the API) | AZ-3658 AC-3 | Accept as an arrangement from outside | clear |
| 6 | AC-2 says the reaper requeues expired jobs with attempt + 1; the contract (`maintenance_ports.md`) says the reaper only marks exhausted jobs dead and the next claim takes over with attempt + 1; the test follows the contract | AZ-3660 AC-2 | Reword the AC | clear |
| 7 | "Interactive lane" in AC-1 is a job priority (0), not a `media-stats` lane | AZ-3660 AC-1 | Reword the AC | clear |
| 8 | A refused entry fails before enqueue (the media effect runs last per entry); an invalid-name entry stands in for "rolled back"; whole-commit rollback is AZ-3655 AC-4 | AZ-3660 AC-1 | Accept or reword | clear |
| 9 | The held long-poll is proven server-side with `max_holds_per_principal=1` (a same-principal probe gets 429) | AZ-3658 AC-1, AC-2 | Keep | clear |

## Commit

`[AZ-3657] [AZ-3658] [AZ-3659] [AZ-3660] Download, feed, tree and media pipelines` — Loop: 1

## Next Batch: cumulative review 86-88, then AZ-4332 (remediation) and a recomputed ready list
