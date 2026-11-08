# Batch Report

**Batch**: 28
**Tasks**: AZ-3746_blob_maintenance_ports, AZ-3872_worker_slot_loop, AZ-3804_version_gate_server_info, AZ-3811_healthcheck_openapi_version
**Date**: 2026-11-08

Four workers on four disjoint crates (`04_blobstore`, `worker`, `11_http_api`, `server`) against one PostgreSQL 18 test container. No worker left its `Owns:` paths. Parent edits: `media-shared` gained `cpu_transcode_plan` (the AZ-3872 worker stopped on the failed-`Remux` rerun because the policy had no way to build that plan; the worker then wired it in) and `media_policy.md` 1.1.0. Deferred: AZ-3803, AZ-3808, AZ-3812 (share `11_http_api` with AZ-3804), AZ-3893 (Windows only), all decision spikes.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3746_blob_maintenance_ports | Done except the tail of AC-1 | `sunnycloud-blobstore`: `rewrap_batch`, `.epoch` marker (`FileEpochMarker`), missing-blob inventory, `BlobHealth` contributor, doctor checks (volume, retention, missing, key), `volume_facts`; `epoch-marker-and-health.md` 1.1.0 | 39 new (301 in the crate) | Items 1-6 |
| AZ-3872_worker_slot_loop | Done; heartbeat belongs to AZ-3873 | `sunnycloud-worker`: `slot_loop/` (`SlotLoop`, `JobRun`, failure mapping, config, `ApiAccess`, metrics); uses `choose_step`, `check_output` and the new `cpu_transcode_plan` | 40 unit + 37 integration | Items 7-10 |
| AZ-3804_version_gate_server_info | Done | `sunnycloud-http-api`: `version/` gate (layer 7, before auth and body), `ServerInfo` with the eight contract fields, `sync_client_versions_total`, 426 with `update_url` | 30 new (28 unit, 1 auth_before_block, 144 it) | Items 11-14 |
| AZ-3811_healthcheck_openapi_version | Done | `sunnycloud-server`: `healthcheck` (GET /readyz on 127.0.0.1:9090), `openapi` (same `current_document()` as 11 serves), `version`, `build.rs` commit stamp; `operator_command_surface.md` 1.3.0 | 7 unit + 5 binary | Items 15-17 |
| (shared) `media_shared::policy::cpu_transcode_plan` | Done | `media-shared`, `media_policy.md` 1.1.0 | 2 | Item 10 |

## Code Review Verdict: PASS_WITH_WARNINGS

No blocking findings. One fixed in this batch: the two new `media-shared` tests pushed `policy_rules.rs` to 514 lines (cap 500), so they moved to `tests/policy_cpu_plan.rs`. No production `unwrap`, `let _ =` or empty catch arm in the diff; every new file is under 500 lines. Open: the version gate also gates `Worker` and `ShareGrant` routes, so once a minimum is set a worker call or a share link without `X-Sunnycloud-Client` gets 400 (the worker client sends no such header); `update_url` has no documented source; AC-1 of AZ-3746 cannot drop key 1 from the ring; `rewrap_batch` scans `blobs` without an index on `master_key_id`.

CI-parity: PASS. In `src/` with `CARGO_INCREMENTAL=0`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`, `cargo deny check`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. After the gate only the `media-shared` test file was moved; fmt, clippy and both `media-shared` policy test binaries were re-run green. Not run: `check-migrations.sh` (needs `sqlx-cli`).

## Test Suite

- Total: 2456 (workspace, default features, real PostgreSQL 18)
- Passed: 2447
- Failed: 0
- Skipped/ignored: 9

## Owner decisions

None needed this batch: no new `run_system` allowlist line (the new statements are in `keyring_repo.rs`, `finding_repo.rs` and `scrub_repo.rs`, already listed). Asked-for later: a dedicated `inventory_repo.rs` would need an allowlist line.

## Issues and discovered scenarios

| # | Scenario or question | Spec ref | Handling | clear / unclear |
|---|----------------------|----------|----------|-----------------|
| 1 | AC-1 tail: remove key 1 from the ring, then `load_and_verify` passes: the ring loader always holds `master_key_file` as id 1 and config rejects any other path for id 1, so key 1 cannot be dropped | AZ-3746 AC-1 | Not built; rest of AC-1 verified (no row under key 1, files byte-identical, new blobs use key 2). Needs an owner decision on ring semantics (AZ-3725, `sc_config`) | unclear |
| 2 | `find_missing(batch)` has no cursor: the inventory keeps the last id in memory and wraps; `scan_page(after, batch)` is stateless and used by the doctor. IT-04-15 says 7 ids over 2 calls but 50 rows at batch 20 need 3 | AZ-3746 AC-4 | Worker's reading; contract text updated | unclear |
| 3 | No stat rate and no `batch` for the missing-blob doctor check: one file at a time on the blocking pool; `batch` is a constructor argument 12 must choose | AZ-3746 | No value invented | unclear |
| 4 | Doctor finding levels: unwritable or missing volume and free space under the floor are Fail, non-XFS/ext4 is Warn; retention 0 and retention shorter than `--backup-interval` are Warn (the "minus GC interval" part of R33 not checked); key check absent while blobs exist is Fail | AZ-3746, observability | Read from `deployment/observability.md`; Fail readings inferred | unclear |
| 5 | The spec excludes the epoch-bump check and metrics (05 and E-10 own them); the brief listed them by mistake | AZ-3746 scope | Not built | clear |
| 6 | `rewrap_batch` selects `WHERE master_key_id = $1 LIMIT n` and `blobs` has no index on `master_key_id`: a full rotation scans per batch. AC-3 volume test uses a raised `blob_free_reserve` (a 64 MiB filled volume needs root); the read-only root test uses chmod 0555 and fails as root | AZ-3746 | Index needs a migration (outside the crate) | unclear |
| 7 | Failed `Remux` reruns once as a CPU `Transcode` via `media_shared::policy::cpu_transcode_plan`; a second `EncoderFailed` or `Stalled` is `fail(Permanent)` | AZ-3872 AC-4 | Done after adding the function | clear |
| 8 | Heartbeat, silence bound and slot-unhealthy outcome are AZ-3873; decoder calls pass `pending()` as the cancel future; status reporter, `/readyz` registration of `ApiAccess` and `JobMetrics::register` are AZ-3874 | AZ-3872 | Left to those tasks | clear |
| 9 | Failure codes with no documented spelling: `MissingDuration` (permanent), `UnknownParamsVersion` (retry), `Protocol`, `ScratchIo`, `DecoderUnreachable`, `ProtocolViolation`, `Busy`, `Cancelled`, `InvalidRequest`, `TypeRejected`; a failure report answered with a non-409/403 error is logged and ignored; the poster thumb uses the video output cap; metric result labels beyond `ok`, `obsolete`, `remuxed` were added | AZ-3872, AZ-3876 open points | Worker's reading | unclear |
| 10 | Log fields `job_id`, `kind`, `attempt`, `slot`, `err_code` are not in the `sc_observability` field schema, so the loop logs `err` only; the input-fit check refuses a small input on a nearly full mount | AZ-3872 | Shared crate untouched | unclear |
| 11 | `HttpConfig.update_url: Option<String>` has no documented source; required when a minimum is set; `sc_error` has no `update_url` field so `ApiProblem::with_update_url` splices it into the 426 body | AZ-3804 open point 2 | Owner decision; 12 or `sc_config` must supply it | unclear |
| 12 | Version gate covers `Worker` and `ShareGrant` routes like every non-`Public` class; once a minimum is set both get 400 without the header and the worker client sends none. One-line change in `VersionGate::guard`; a test pins the current behaviour | AZ-3804 | Owner decision, high impact | unclear |
| 13 | Absent header with bearer or no credentials is 400 `invalid_request`; a family missing from a `family=semver,...` list has no minimum; with no minimum set nothing is refused but a malformed header is still 400; `ServerInfo.min_client_version` is a required `String`, so 12 decides what an unset key serves; `longpoll_max_wait` is whole seconds | AZ-3804 open points 1, 3, 4 | Worker's reading | unclear |
| 14 | Interface changes for wiring: `ServerInfo` has five pub fields and no `Default`; `HttpConfig` gained `update_url`; `RouteContext` gained `version_gate`; `HttpServer::new` returns `InvalidConfig` for a bad minimum, a minimum without `update_url`, or a `server_info.min_client_version` that differs from `min_client_version` | AZ-3804 | For AZ-3789 | clear |
| 15 | `healthcheck` has no probe timeout (no doc or config key gives one): Compose's timeout or SIGINT ends a silent peer | AZ-3811 flagged concern | No value invented (open point 10) | unclear |
| 16 | `openapi --json` prints the bare document, not the `{result, code, data}` envelope, so bytes equal the served document; no trailing newline added | AZ-3811 AC-2 | Open point 11 | unclear |
| 17 | `healthcheck` address is fixed at `127.0.0.1:9090` although `ops_addr` is configurable | AZ-3811 | Open point 12 | unclear |
| 18 | Items open from batches 18-27 stay open (purger lock strength, Retry-After, contracts change tickets, sc_error rows for 10 port variants, serde on the contracts media model, `utoipa` and a validator, CI wiring of `openapi_diff`, `http_panics_total`, media policy open point 3, session cookie name and device prefix, scrub test debug-build time, scrub `batch` size, AZ-3726 privileged rows, AZ-3837 AC-4) | earlier batches | | unclear |

## Commit

`[AZ-3746] [AZ-3872] [AZ-3804] [AZ-3811] Blob maintenance ports, worker slot loop, version gate, local CLI commands` — Loop: 1

## Next Batch

Recompute ready tasks. Likely ready: one of AZ-3803 body limits, AZ-3808 CSRF origin check, AZ-3812 content header mapper (all `11_http_api`, only one at a time); AZ-3873 and AZ-3874 (worker) and AZ-3747 and AZ-3748 (blobstore), if their dependencies are now done. 266 tasks left.
