# Batch Report

**Batch**: 30
**Tasks**: AZ-3808_csrf_origin_check, AZ-3874_worker_status_shutdown_main, AZ-3779_epoch_admin
**Date**: 2026-11-08

Three workers on three disjoint crates (`11_http_api`, `worker`, `05_sync_feed`) against one PostgreSQL 18 test container. No worker left its `Owns:` paths. Parent edits before the workers started: `sc_error` row `csrf_rejected` (403) with its test, `sc_error.md`, helper 02 and the sync-core wire table (named by the AZ-3808 spec and `api_v1_surface.md`). Owner-approved this batch: allowlist line `src/components/05_sync_feed/src/epoch_repo.rs`. Addendum to batch 29 (same day, owner-approved): `length_required` (411) and `download_write_idle`, commit 43f5602. Deferred: AZ-3812, AZ-3831 (share `11_http_api` with AZ-3808), AZ-3877 (shares `worker` with AZ-3874), AZ-3893 (Windows only), all decision spikes.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3808_csrf_origin_check | Done | `sunnycloud-http-api`: `csrf/` (`CsrfGuard`, origin canonicalisation), layer 9 after auth and before body limits, `RouteContext.csrf`, `public_url` validated by `HttpServer::new`; `api_v1_surface.md` and `http_server.md` 1.2.0 | 7 unit + 12 integration | Items 1-5 |
| AZ-3874_worker_status_shutdown_main | Done except `thumbnail_ready_seconds` and the spec's log fields | `sunnycloud-worker`: `compose.rs`, `status.rs`, `slot_loop/shutdown.rs` (`Shutdown` drain then abort), `run_worker`, `JobEnd::Released`, kill counters, build-timestamp gauge | 31 new + 1 | Items 6-12 |
| AZ-3779_epoch_admin | Done except the `DoctorCheck` type, the old/new epoch in the INFO log, and AC-2 on a read-only root | `sunnycloud-sync-feed`: `epoch_repo.rs` (`PgEpochAdmin`), `epoch_check.rs` (`epoch_findings`); `epoch-admin.md` 1.0.0 | 13 new | Items 13-17 |

## Code Review Verdict: PASS_WITH_WARNINGS

No blocking findings. No production `unwrap`, `expect`, `let _ =` or empty catch arm in the diff (the one `expect` is in a test module); every changed file is at most 500 lines. I read the CSRF layer in full: the decision is made on the `Authenticated` extension, so a bearer wins over a cookie, a bad cookie gets the auth 401, and the `Host` header is never read. Warnings: the three `epoch_resync` tests called a helper `now()`, which `check-no-sql-now` reads as SQL; renamed to `bump_time()` after the gate. Open items below.

CI-parity: PASS. In `src/` with `CARGO_INCREMENTAL=0`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`, `cargo deny check`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. The full run had one failure (`check-no-sql-now`, three false positives, one cause, fixed); after the fix the script, fmt, clippy for `sunnycloud-sync-feed` and its 20 epoch tests were re-run green. Not run: `check-migrations.sh` (needs `sqlx-cli`).

## Test Suite

- Total: 2565 (workspace, default features, real PostgreSQL 18)
- Passed: 2556
- Failed: 0
- Skipped/ignored: 9

## Owner decisions

Approved this session: allowlist line `src/components/05_sync_feed/src/epoch_repo.rs` (`bump_epoch`, `marker_status` and `head_report` read and write the feed rows of every account; operator path, ST-05-2).

## Issues and discovered scenarios

| # | Scenario or question | Spec ref | Handling | clear / unclear |
|---|----------------------|----------|----------|-----------------|
| 1 | "Or" semantics: `Sec-Fetch-Site: cross-site` plus an `Origin` equal to `public_url` passes (a browser never sends that pair). Refusing any non-`same-origin` `Sec-Fetch-Site` would be stricter | AZ-3808, open point 15 | Worker's reading of the contract | unclear |
| 2 | `ShareGrant` routes are not checked: 06 resolves the grant in the handler and no `Authenticated` is produced. If a grant-cookie route ever mutates state the owners must decide | AZ-3808, open point 14 | Open | unclear |
| 3 | Whether the web app's refresh request is cookie-authenticated, and so under CSRF, is not stated by 01 | AZ-3808 flagged concern, open point 16 | Open | unclear |
| 4 | GET provider callback: the layer checks no GET or HEAD, so it needs no exemption; the registry validator already names `SignInService::complete` as the one GET allowed to be state-changing. The route is not in `production_routes()` yet, so AC-5 is tested over test routes plus a registry assertion. `Origin: https://host:443` equals `https://host`; a repeated `Origin` or `Sec-Fetch-Site` header proves nothing | AZ-3808 | Worker's reading | unclear |
| 5 | `RouteContext` gained the public field `csrf`; code that builds it by hand breaks, and 12 must compose the server through `HttpServer::new` | AZ-3808 | For AZ-3789 / 12 | clear |
| 6 | `thumbnail_ready_seconds` is not registered: the spec's own flagged concern says it has no defined start point, and a histogram never observed would be a stand-in | AZ-3874 | Needs an E-20 owner decision | unclear |
| 7 | The spec's log fields `job_id`, `kind`, `attempt`, `slot`, `err_code` are not in the `sc_observability` field schema (only `request_id`, `account_id`, `node_id`, `route`, `status`, `duration_ms`, `err`), so the worker logs `err` only | AZ-3874 | Shared crate change, owner decision | unclear |
| 8 | `worker_sock_*`, `worker_scratch_*` and `decoder_gid` are `Opt` with no default in `sc_config` while the 09 doc lists defaults for the first four; an unset one is a startup error (exit 3). Either defaults go into the table or the doc changes | AZ-3874 | Owner decision | unclear |
| 9 | Decoder reconnect backoff has no config key; named constants (1 s initial, 30 s max) in `compose.rs`. Batch 16 suggested `worker_decoder_reconnect_initial` and `worker_decoder_reconnect_max`. `self_check` forbidden targets: only the `host:port` of `worker_api_url` is passed, the worker knows no database or internet target | AZ-3874 | Owner decision | unclear |
| 10 | `image_age_days` is whole days rounded down from the oldest decoder that has said `hello`, 0 when none has; the first report is sent at start and may say `decoder_ok=false` before the gates pass. Kill reasons: stall is `Stalled` or a silent decoder, timeout is `LimitExceeded{Wall}` or the wall watchdog, memory is `LimitExceeded{Memory}`; a closed socket is not counted (10_decoder says socket loss maps to `memory`) | AZ-3874 | Worker's reading | unclear |
| 11 | Metric labels: `worker_selfcheck_ok`, `worker_decoder_restarts_total` and `worker_decoder_build_timestamp_seconds` carry a `slot` label where `observability.md` lists none; the job result list there should read `ok`, `remuxed`, `skipped_compliant`, `obsolete`, `retry`, `permanent`, `lease_lost`, `refused`, `released`. No contract doc for 09 exists to update | AZ-3874 | Doc follow-up | unclear |
| 12 | After the abort there is no hard bound: the wait is `decoder_cancel_grace` plus the release call, and `WorkerApiClient` has no request timeout, so a hung API holds the exit until SIGKILL (lease expiry then returns the job). The `test-clock` feature is still a placeholder (E-20/c09) | AZ-3874 AC-4 | Left | unclear |
| 13 | `DoctorCheck` type for the epoch check is not built: nothing names where the doctor learns "the volume holds blobs" (05 must not read 04's tables). `epoch_findings(status, holds_blobs)` exists and is tested (Fail, Fail, Ok, Ok); the struct would be about 30 lines once a source (port injected by 12, or an extension of `EpochMarker`/`BlobInventory`) exists | AZ-3779 AC-6, flagged concern | Owner decision | unclear |
| 14 | The real 04 `FileEpochMarker` cannot be a dev-dependency of 05: `src/server/tests/crate_graph.rs` counts normal and dev dependencies and rejects any component-to-component edge. The tests use a shared `MarkerDouble`; the read-only blob root of AC-2 and any real-file assertion are not covered here and belong to a wiring test in `server`/12 or to 04's tests | AZ-3779 AC-2, flagged concern | Open | unclear |
| 15 | The INFO log `epoch bumped` carries no old and new epoch: the log schema has no epoch fields. Two i64 fields (for example `old_epoch`, `new_epoch`) in `sc_observability` would close it | AZ-3779 | Shared crate change, owner decision | unclear |
| 16 | Spec inconsistency: the new epoch is `greatest(max(epoch)+1, unix_seconds)` for every row (AC-1: the same epoch on all rows), identical to the per-row formula when all rows share an epoch. With no feed row at all `bump_epoch` returns `NothingToBump` and writes no marker, so an empty restored database with a marker present cannot be fixed by `epoch-bump`; `marker_status` and `head_report` report epoch 0 | AZ-3779 | Documented in `epoch-admin.md` | unclear |
| 17 | `marker_status` returns a marker read failure as `Unexpected` although the part2 contract table says "errors: none"; a bump running concurrently with account creation can leave the new account on the old epoch (no lock); the restore in AC-5 and AC-7 is simulated by setting `seq`, no real `pg_dump` | AZ-3779 | Worker's reading | unclear |
| 18 | Items open from batches 18-29 stay open (the slow-body status, purger lock strength, Retry-After, contracts change tickets, `sc_error` rows for 10 port variants, serde on the contracts media model, `utoipa` and a validator, CI wiring of `openapi_diff`, `http_panics_total`, media policy open point 3, session cookie name and device prefix, scrub test debug-build time, key ring semantics for retiring key 1, version gate on `Worker`/`ShareGrant`, `update_url`, `healthcheck` timeout and address, AZ-3726 privileged rows, AZ-3837 AC-4, feed hook scope for 01) | earlier batches | | unclear |

## Commit

`[AZ-3808] [AZ-3874] [AZ-3779] CSRF origin check, worker main and shutdown, epoch admin` — Loop: 1

## Next Batch

Recompute ready tasks. Likely ready: one of AZ-3812 content header mapper or AZ-3831 internal router and test clock (`11_http_api`, only one at a time); AZ-3877 worker nvenc preference (`worker`); tasks unlocked by the epoch admin, the CSRF layer and the worker main. 259 tasks left.
