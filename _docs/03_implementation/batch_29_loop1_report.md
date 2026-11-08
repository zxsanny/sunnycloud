# Batch Report

**Batch**: 29
**Tasks**: AZ-3748_blobstore_metrics_logging, AZ-3778_feed_horizon_lifecycle, AZ-3803_body_limits_timeouts, AZ-3873_worker_heartbeat_watchdog
**Date**: 2026-11-08

Four workers on four disjoint crates (`04_blobstore`, `05_sync_feed`, `11_http_api`, `worker`) against one PostgreSQL 18 test container. No worker left its `Owns:` paths and no shared crate was edited this batch: two parts of AZ-3803 stopped on a shared-crate change (items 9 and 10) and wait for the owner. Deferred: AZ-3808, AZ-3812 (share `11_http_api` with AZ-3803), AZ-3877 (shares `worker` with AZ-3873), AZ-3893 (Windows only), all decision spikes.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3748_blobstore_metrics_logging | Done | `sunnycloud-blobstore`: `metrics.rs` (`BlobMetrics`), `with_metrics` builders on `SyncBarrier`, `FileBlobReader`, `BlobHealth`; new ERROR/WARN events (corrupt read, write I/O, group sync, floor hit, aborted GC sweep); `metrics-and-logs.md` 1.0.0 | 9 new (310 in the crate) | Items 1-4 |
| AZ-3778_feed_horizon_lifecycle | Done; AC-3 uses a marker double | `sunnycloud-sync-feed`: `horizon_repo.rs` (`PgFeedHorizon`), `lifecycle_repo.rs` (`PgFeedLifecycle`); `feed-writer-ports.md` 1.1.0 | 9 new | Items 5-7 |
| AZ-3803_body_limits_timeouts | Done except 411, download write-idle, slow-body status | `sunnycloud-http-api`: `limits/` (`BodyLimits`, per-route layer, streaming guard), `Timing` on `RouteSpec`, `worker_output_limits` in `HttpConfig`; `api_v1_surface.md` and `http_server.md` 1.1.0 | 13 integration + 6 unit | Items 8-12 |
| AZ-3873_worker_heartbeat_watchdog | Done | `sunnycloud-worker`: `slot_loop/lease.rs` (heartbeat at `lease_ms / 3`), `watchdog.rs` (silence, wall limit, lease end), `Stop::Unhealthy`, `Slot::mark_unhealthy`, readiness via the existing `SlotReadiness` | 9 new | Items 13-16 |

## Code Review Verdict: PASS_WITH_WARNINGS

No blocking findings. No production `unwrap`, `expect`, `let _ =` or empty catch arm in the diff; every changed file is at most 419 lines. I read the watchdog and the body-limit layer in full: the select order and the cancel-then-grace flow are right. Warnings: a request cut at the idle or request timeout is answered with a 500 head followed by a reset, so it counts as a 5xx in the request metrics until the status question (item 10) is decided; the `output_kind` path-parameter failure in the limits layer drops the extractor's reason and returns a generic unexpected error.

CI-parity: PASS. In `src/` with `CARGO_INCREMENTAL=0`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`, `cargo deny check`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. All returned 0. Not run: `check-migrations.sh` (needs `sqlx-cli`).

## Test Suite

- Total: 2502 (workspace, default features, real PostgreSQL 18)
- Passed: 2493
- Failed: 0
- Skipped/ignored: 9

## Owner decisions

None asked this batch: no new `run_system` allowlist line (the feed tests use the already listed `system_scope_repo.rs`). Waiting for the owner: the 411 code and the download write-idle key (items 9, 10).

## Issues and discovered scenarios

| # | Scenario or question | Spec ref | Handling | clear / unclear |
|---|----------------------|----------|----------|-----------------|
| 1 | What one `blob_read_duration_seconds` observation covers: one successful `open`, timed to the first authenticated chunk; label `kind` is `range` for an open with a byte range, `stream` without; `read_head` and failed opens are not observed | AZ-3748 flagged concern | Worker's reading | unclear |
| 2 | The new blobstore log lines carry the blob id inside `err`: `sc_observability` has no typed `blob_id` field, and the reader and writer do not know the account, so no account id is logged | AZ-3748 AC-4 | A `blob_id` field would be a shared-crate change | unclear |
| 3 | Only `BlobError::Io` is logged as a write I/O error; `NoSpace` from a mid-write `pwrite` is not | AZ-3748 | Worker's reading | unclear |
| 4 | Wiring left to the composition root (AZ-3789): `BlobMetrics::register`, the three `with_metrics` calls, and a periodic `probe` (or a `ScrapeProbe`) so the two gauges move outside `/readyz` polling | AZ-3748 | For AZ-3789 | clear |
| 5 | `EpochMarker` lives in `contracts`, so 05 has no edge to 04, but the tests use an in-memory marker double, not the real file marker on a temp directory (spec AC-3 blackbox). The real marker needs a 05 to 04 dev-dependency the crate-graph test may forbid, or the file implementation moved to a shared crate | AZ-3778 AC-3, flagged concern | Open | unclear |
| 6 | `account_feeds` has row-level security: `initialize` only sees the installation epoch from a system-scope transaction, otherwise a second account silently gets the clock epoch. 01 must call the hook in system scope; not verified. The alternative is a SECURITY DEFINER function or policy change (a migration) | AZ-3778 | Documented in the contract; owner/01 | unclear |
| 7 | Two first accounts created at the same instant on an empty install could pick different epochs (no lock taken); the marker is written inside the caller's transaction, so a rollback leaves a marker no row has (the spec's own Low concern); hook registration order after the tree's is 12's | AZ-3778 | Documented | unclear |
| 8 | A `Timing` marker (`Request`, `LongPoll`, `Download`) was added to `RouteSpec`; later routes for changes and downloads must declare `.long_poll()` and `.download()`. `wait` is read in whole seconds. `Block` and `Staged` have no total cap; `WorkerOutput`, `Commit` and `Batch` get `request_timeout` (a large transcode upload is bounded at 60 s) | AZ-3803 | Worker's reading | unclear |
| 9 | Block PUT without `Content-Length` is not answered 411: `sc_error` has no 411 code (proposed `length_required`, 411, "Length required"); until it exists such a PUT is streamed and cut at 2 MiB | AZ-3803 AC-2, open point 13 | Needs an `sc_error` row, wire vocabulary: owner decision | unclear |
| 10 | Download write-idle timeout (60 s) is not enforced: `sc_config` has no key (proposed `download_write_idle`, duration, 60s, HTTP section); a slow body and a slow handler end as a reset behind a 500 head because `problem_details.md` lists no 408 and no request-timeout code | AZ-3803 AC-4, AC-5, open points 11, 12 | Owner decisions | unclear |
| 11 | The layer's 413 for a worker output over its cap is `size_too_large`, while 08 answers `output_too_large` (also 413); the layer enforces the cap as a ceiling and 08 keeps its own check. The `POST revisions:lookup` body class is still unassigned. Not tested: HTTP/2 reset, memory growth for AC-6; 413 tests assert 500 ms, not 100 ms | AZ-3803 | Worker's reading | unclear |
| 12 | `HttpConfig` gained `worker_output_limits` (from the existing `derived_max_*` keys) and rejects a zero `request_timeout` or `body_idle_timeout`; `RouteContext` gained `body_limits` | AZ-3803 | For AZ-3789 | clear |
| 13 | The wall limit is applied per decoder call (probe, each render, each transcode, the CPU rerun), so a two-render `stills` job can run twice the limit; the runner's own limit and our cancel can race at the same instant | AZ-3873 AC-4 | Worker's reading | unclear |
| 14 | Silence and wall limit always mark the slot unhealthy and fail the job `Retry`, even when the decoder answers the cancel inside the grace; a lost lease during a call sends `cancel` and waits the grace but does not mark the slot unhealthy | AZ-3873 AC-2, AC-3 | Worker's reading | unclear |
| 15 | Heartbeat interval is fixed at `lease_ms / 3` from the claim and ignores the `lease_ms` of the extension reply; a protocol violation maps to `Unhealthy`, a runner exit or I/O error stays a plain reconnect through the gate | AZ-3873 | Worker's reading | unclear |
| 16 | Not tested: lease loss during input fetch or output post (covered by code in `while_leased`); harness H2 cases belong to E-20/c11. Registering `SlotLoop::readiness()`, `ApiAccess` and `JobMetrics::register` and the status reporter stay with AZ-3874 | AZ-3873 | Left to AZ-3874 | clear |
| 17 | Items open from batches 18-28 stay open (including the version gate and `Worker`/`ShareGrant` routes, `update_url`, key ring semantics for retiring key 1, `healthcheck` timeout and address) | earlier batches | | unclear |

## Commit

`[AZ-3748] [AZ-3778] [AZ-3803] [AZ-3873] Blobstore metrics, feed horizon, body limits, worker watchdog` — Loop: 1

## Next Batch

Recompute ready tasks. Likely ready: one of AZ-3808 CSRF origin check or AZ-3812 content header mapper (`11_http_api`, only one at a time); AZ-3877 worker nvenc preference; tasks unlocked by the feed horizon, the metrics and the watchdog. The 411 code and the download write-idle key (items 9, 10) can be added once the owner confirms the names. 262 tasks left.
