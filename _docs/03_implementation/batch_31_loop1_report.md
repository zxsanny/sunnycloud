# Batch Report

**Batch**: 31
**Tasks**: AZ-3812_content_header_mapper, AZ-3877_worker_nvenc_preference, AZ-3780_feed_security_acceptance
**Date**: 2026-11-11

Three workers on three disjoint crates (`11_http_api`, `worker`, `05_sync_feed`) against one PostgreSQL 18 test container. No worker left its `Owns:` paths; no shared crate was edited. Deferred: AZ-3831 and AZ-3828 (share `11_http_api` with AZ-3812), AZ-3893 (Windows only), all decision spikes.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3812_content_header_mapper | Done except the 416 `Content-Range` (blocked on a `contracts` change) | `sunnycloud-http-api`: `content/` (`content_response`, `disposition.rs`), `content_serving_headers.md` 1.0.1; `http-body-util` moved to dependencies | 11 unit + integration over TCP incl. h1 close and h2 RST_STREAM | Items 1-4 |
| AZ-3877_worker_nvenc_preference | Done | `sunnycloud-worker`: `choose_step(.., nvenc_enabled)` and `encoder_pref`, `LoopConfig.nvenc`, `WorkerError::Nvenc`, strict `on`/`off` in `load_settings` | unit matrix, 4 combinations through the fake decoder, CPU rerun after an `Nvenc` plan | Items 5-6 |
| AZ-3780_feed_security_acceptance | AC-1..5 done; AC-6, AC-7 not covered; AC-8 partly | `sunnycloud-sync-feed`: `feed_resync_required_total` counter in `PgChangeFeed` (new constructor parameter), `change-feed.md` 1.0.1; new security, exhaustion and log-audit tests | `security_scoping`, `security_injection`, `security_revoked`, `resync_metric`, `hold_exhaustion` (full size), `feed_log_audit` | Items 7-12 |

## Code Review Verdict: PASS_WITH_WARNINGS

No blocking findings. No production `unwrap`, `expect`, `let _ =` or empty catch arm in the new code; every new file is at most 297 lines. I read the `PgChangeFeed::read` wrapper, `encoder_pref` and its wiring, and the worker diffs. Warnings: AZ-3780 leaves two acceptance criteria and one clause uncovered (items 7, 8); AZ-3812's 416 answer lacks `Content-Range`, `ETag` and `Accept-Ranges` (item 1).

CI-parity: all gates passed except `cargo deny` (item 13). In `src/` with `CARGO_INCREMENTAL=0`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. Not run: `check-migrations.sh` (needs `sqlx-cli`). The first gate run was invalid (disk full, link failures); it was repeated from a clean build after the disk was freed.

## Test Suite

- Total: 2603 (workspace, default features, real PostgreSQL 18)
- Passed: 2594
- Failed: 0
- Skipped/ignored: 9

## Owner decisions

None asked this batch. Waiting for the owner: items 1, 5, 6, 7, 8, 9, 13 below.

## Issues and discovered scenarios

| # | Scenario or question | Spec ref | Handling | clear / unclear |
|---|----------------------|----------|----------|-----------------|
| 1 | `DownloadError::RangeNotSatisfiable` is a unit variant, so 11 does not know the total size: the 416 carries no `Content-Range: bytes */<total>`, `ETag` or `Accept-Ranges`. Needs `RangeNotSatisfiable { total_size: i64 }` in `contracts`, `part2_tree_transfer_feed.md`, `download_service.md`, and a small update in `port_error/files.rs`. Today the 416 is the plain problem body with `nosniff` | AZ-3812 AC-3 | Not stubbed; owner approval for a contracts change | unclear |
| 2 | An `Inline` answer for a type outside the seven-type allowlist, or without a type, is rendered as an attachment (octet-stream, sandbox CSP); matching is case-insensitive | AZ-3812 | Worker's reading | unclear |
| 3 | `ETag` is 03's `etag` wrapped in double quotes unless it already starts with one (the contract does not say). File names: control and bidi characters dropped, `/` and `\` become `_`, cut to 255 UTF-8 bytes, empty or dots-only becomes `download`, no plain `filename=` fallback. A broken 03 answer (no valid range, negative total, unusable etag) is logged and answered as an opaque 500. 304 carries the content headers minus `Content-Length` | AZ-3812 | Worker's reading | unclear |
| 4 | Not covered: the 90% coverage figure (no tool run), PT-11-1 (memory while streaming), `Access-Control-Allow-Origin` for public share content (belongs to the share route, c14). The test routes are test-local; the real content routes (c13, c14) do not exist yet | AZ-3812 | Left | clear |
| 5 | `nvenc` accepts exactly `on` or `off`; anything else (`ON`, `true`, `1`, `auto`, empty) is a startup error, exit code 3. The alternative is treating any non-`on` as off | AZ-3877 | Worker's reading | unclear |
| 6 | "A non-compliant probe" in AC-1 is read as a clip that fails the compliance check (so a transcode is chosen), not as a non-compliant `hello`. The matrix runs on a transcode-needing clip | AZ-3877 AC-1 | Worker's reading | unclear |
| 7 | AC-6 (IT-05-7 b) and AC-7 (AT-05-3) need the real 01 revalidator and `revoke_device`; `crate_graph.rs` forbids a 05 to 01 dev-dependency (the spec's own flagged concern). Could live in `src/server/tests/` (the server crate depends on both) or in AZ-3781. AC-5 is covered with a scripted revalidator, as the spec allows | AZ-3780 AC-6, AC-7 | Not stubbed; owner to confirm where they move | unclear |
| 8 | AC-8: the INFO line on an epoch bump carries no old or new epoch (the `sc_observability` fields are `request_id`, `account_id`, `node_id`, `route`, `status`, `duration_ms`, `err`, and the macro takes a literal message). Needs typed `old_epoch` and `new_epoch` fields and `bump_epoch` returning the old value. The audit shows no feed log line carries a name or path | AZ-3780 AC-8 | Shared-crate change, owner approval | unclear |
| 9 | The `resync_required` metric had no name; the worker added `feed_resync_required_total` (unlabelled). It has no per-account label because labels must be a static enumeration, so the spec's "per account" is not met. `PgChangeFeed::new` gained a parameter: the composition root (AZ-3789) must pass the counter | AZ-3780 flagged concern | Name needs owner confirmation | unclear |
| 10 | The immediate read path never calls the revalidator, only the held path does: after a revoke an item page can leave on the immediate path until the next request is authenticated, so the bound there is 01's session cache time at the HTTP layer. Stated in `change-feed.md` | AZ-3780 AC-5 to AC-7 | Documented; design change if 05 should enforce it | unclear |
| 11 | A refused read has already made its one short first-page query before the cap check, so "no connection" holds for parked reads but not for the refusal itself; within the pool bound in ST-05-3. Measured at full size: 1,000 reads from one principal gave 4 parked and 996 `TooManyHolds`; 100 principals gave 400 parked and 9,600 refused; RSS grew by 6.7 MB and 10.5 MB (budget 32 MB) | AZ-3780 AC-3 | Worker's reading | unclear |
| 12 | "`bump_epoch` and `marker_status` reachable only in system scope" has only indirect evidence (the allowlist and `check-db-gates`, an unscoped bump under B reaches only B's row); no scope-specific test was added | AZ-3780 AC-2 | Left | unclear |
| 13 | `cargo deny check` fails with RUSTSEC-2026-0459 (`libsqlite3-sys` 0.37.0 through `rusqlite` 0.39.0 in `sunnycloud-daemon`; fix is `libsqlite3-sys` 0.39.0 or later). The lock entry is not part of this batch (the lock gained only `futures-core`); the advisory is new since the last gate. Not fixed: a dependency bump in the desktop daemon is outside this batch | gate | Owner decision | unclear |
| 14 | Items open from batches 18-30 stay open (see batch 30 report) | earlier batches | | unclear |

## Commit

`[AZ-3812] [AZ-3877] [AZ-3780] Content header mapper, worker NVENC preference, feed security suite` — Loop: 1

## Next Batch

Recompute ready tasks. Likely ready: AZ-3831 internal router and test clock or AZ-3828 identity admin handlers (`11_http_api`, only one at a time); tasks unlocked by the header mapper. 256 tasks left.
