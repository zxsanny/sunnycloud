# Batch Report

**Batch**: 24
**Tasks**: AZ-3810_error_mapper, AZ-3728_blob_reader, AZ-3777_feed_long_poll, AZ-3837_media_shared_params_protocol
**Date**: 2026-11-07

Four workers on four disjoint paths (`11_http_api`, `04_blobstore`, `05_sync_feed`, `src/media-shared`) against one PostgreSQL 18 test container. No worker left its `Owns:` paths. Parent edits: `ChangeFeedError::TooManyHolds { retry_after }` in `src/contracts/src/feed.rs` with a contract test (the approved decision of 2026-11-07, added first so the feed and mapper workers built against it; an earlier doc comment naming "429" broke the contracts test that forbids HTTP status numbers and was reworded), and one owner-approved allowlist line. AZ-3696, AZ-3730, AZ-3743 deferred (same crate or cap); AZ-3893 deferred (Windows only).

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3810_error_mapper | Done; AC-4 holds with 10 port variants that have no `sc_error` row | `sunnycloud-http-api`: `ApiProblem`, `PortError` (one exhaustive match per port error enum), `problem_schema`, OpenAPI `Problem` schema | 20 new (47 in the `it` binary) | Items 1-3 |
| AZ-3728_blob_reader | Done | `sunnycloud-blobstore`: `FileBlobReader` (`reader/`: header, key, per-chunk tag check, read-ahead window, findings) | 17 new + the 1 GiB test (`reader_gib`, release run) | Items 4-6 |
| AZ-3777_feed_long_poll | Done; PT-05-3 and PT-05-4 NOT-RUN(HW-REF) | `sunnycloud-sync-feed`: `PgChangeFeed`, `InProcessNotifier`, `HoldCap`, hold loop | 60 feed + 7 lib + 4 standalone; 4 ignored | Items 7-10 |
| AZ-3837_media_shared_params_protocol | Done except AC-4 (leftover) | `media-shared`: `Params` v1 snapshot, `OutputSpec`, `JobSpec`, `JobKind::lane/outputs` | 14 new (142 in the crate) | Items 11-14 |

## Code Review Verdict: PASS_WITH_WARNINGS

8 findings, none blocking: the device-grant poll answers (`authorization_pending`, `slow_down`, `access_denied`, `expired_token`) have no `sc_error` row and answer a 500 with an ERROR log per poll; about 15 doubtful port variants are mapped to a similar code (meta-rule: no close-enough stand-in); reader chunk reads share the unbounded runtime blocking pool with the group-commit `fdatasync`; one blocking hop and allocation per 64 KiB chunk; the reader awaits the finding write before it sends the error; a refused 5th hold still costs one query; the `contracts` copies of the job types are not compared with `media-shared` byte for byte; the 1 GiB streaming test is ignored in debug CI.

CI-parity: PASS. In `src/` with `CARGO_INCREMENTAL=0`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`, `cargo deny check`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. The first workspace test run failed one test, `contracts --test listing` (my doc comment on `TooManyHolds` named an HTTP status number); fixed, contracts tests 46 of 46, then fmt, workspace clippy and the four scripts re-run green. `check-db-gates` failed in that run only because the owner-approved allowlist line was added after the run started; it passes. Not run: `check-migrations.sh` (needs `sqlx-cli`).

## Test Suite

- Total: 2021 (workspace, default features, real PostgreSQL 18)
- Passed: 2012 in the full run + the 1 `contracts` test after the fix (2013)
- Failed: 0
- Skipped/ignored: 9 (PT-05-1, PT-05-2, PT-05-3, PT-05-4, the AC-9 timing test, the 1 GiB debug test and earlier skips)

## Owner decisions

Approved this session: allowlist line `src/components/04_blobstore/src/reader/finding_repo.rs` (the reader's open call carries no account id, so marking a damaged blob unavailable runs by primary key in system scope).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | Ten port variants have no `sc_error` row: `DeviceGrantError::{AuthorizationPending, SlowDown, AccessDenied, ExpiredToken}`, `SignInError::{InvalidCode, IdentityTaken, InvitationRequired}`, `UploadError::ClientKeyMismatch`, `VaultError::{VaultExists, PreconditionFailed}`; suggested `identity_taken` 409, `invalid_code`, `client_key_mismatch` 409, 412 and 428 for vault; the RFC 8628 answers also need a body shape | AZ-3810 AC-4 | Owners E-15, E-03, E-20 add rows; a new code breaks the exhaustive match in `sync-core/src/wire_table.rs` | unclear, blocks AC-4 strictly |
| 2 | Provisional mappings: `IsRoot`, `UploadNotFound`, `StorageFull`, `WrongPassword`, `Expired`, `AlreadyDecided`, `UnknownCode`, `ReuseDetected`, `UnknownProvider`, `InvalidReturnPath`, `HostMismatch`, `ExpiryOutOfRange`, `InvalidLimit`, `ConfirmationMismatch`, `IndexOutOfRange`, `UntrustedState`; each is a one-line change in `src/port_error/` pinned by a test | AZ-3810 | Review | unclear |
| 3 | 503 without `retry_after` omits header and field; `update_url` of `upgrade_required` and the `current_revision` of `EditedSince` have no slot in `ProblemBody`; HSTS and Referrer-Policy belong to E-21/c04 | AZ-3810 | Owner decisions | unclear |
| 4 | `read_head` above 64 KiB is `OutOfRange` (caller bug), not `RangeNotSatisfiable` | AZ-3728 AC-3 | Owner pick | unclear |
| 5 | "Once per blob per process run": the database is the guard (`unavailable_at IS NULL`), so a blob already marked, also by scrub, writes no second event even after a restart | AZ-3728 | Owner OK | unclear |
| 6 | The 1 GiB test needs `cargo test --release -p sunnycloud-blobstore --test reader_gib` in CI | AZ-3728 AC-7 | CI decision | unclear |
| 7 | `Retry-After` 5 s is `PROPOSED_HOLD_RETRY_AFTER`, not agreed | AZ-3777 | Owner | unclear |
| 8 | No typed unavailable variant in `ChangeFeedError`; an outage is `Unexpected(DbError::Unavailable)` | AZ-3777 | Contracts variant if wanted | unclear |
| 9 | AC-8 pauses the database by cutting a TCP forwarder, not the shared container | AZ-3777 | Keep | clear |
| 10 | The composition root (12) builds one `Arc<InProcessNotifier>` for 02 and the feed; PT-05-3 passed in smoke, PT-05-4 p95 256 ms unmeasured (debug, shared database) | AZ-3777 | Run on REF-SRV | unclear |
| 11 | AC-4 (contracts copies vs media-shared) needs serde derives in `contracts/src/media/model.rs`; leftover `2026-11-07_az-3837-ac4-wire-comparison.md` | AZ-3837 | Contracts owner | unclear |
| 12 | CRF, x264 preset, WebP quality have no values in the spec; left out of `Params` instead of invented; "separate HEIC profile" not modelled | AZ-3837 | Owner decisions | unclear |
| 13 | `JobSpec` fields chosen from data model 9.1: account, revision, kind, content hash, params version | AZ-3837 | Review | unclear |
| 14 | `worker_job_protocol.md` header still says draft; its open point 5 (codes for the 413 and 422 output errors) is open | AZ-3837 AC-5 | Doc owner | unclear |
| 15 | Items open from batches 18-23 stay open (purger lock strength, contracts change tickets) | earlier batches | | unclear |

## Commit

`[AZ-3810] [AZ-3728] [AZ-3777] [AZ-3837] error mapper, blob reader, feed long poll, media shared protocol` — Loop: 1

## Next Batch

Recompute ready tasks. Likely ready: AZ-3730 GC sweeper, AZ-3743 orphan sweeper, AZ-3696 sharing migration and config, plus tasks unlocked by the reader, the mapper and the long poll. 282 tasks left.
