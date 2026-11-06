# Batch Report

**Batch**: 15
**Tasks**: AZ-3687_device_grant_exact_once, AZ-3870_worker_scratch, AZ-3961_sync_hash_file, AZ-3965_feed_client
**Date**: 2026-11-06

Three workers on disjoint crates (`components/01_identity`, `worker`, `sync-core`, the last doing two tasks one after the other), plus a follow-up worker for AZ-3687 after two owner decisions. Owner decisions in this batch: GeoIP format MaxMind mmdb with the `maxminddb` reader (`=0.32.0`); add `Forbidden` to `DeviceGrantError`. Parent edits: the readers of `decoder_gid`, `decoder_max_output_photo` and `decoder_max_output_video` widened from the decoder to the worker (`WR`) in the key table, with a helper-01 note, because AZ-3870 stopped at their absence. No allowlist, advisory or schema change in this batch (the device-grant system-scope doors sit in the already allowlisted `auth_repo.rs`).

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3687_device_grant_exact_once | Done | `sunnycloud-identity`: `IdentityDeviceGrants` (all seven `DeviceGrantService` methods), `device_grant_repo.rs`, four thin system-scope doors in `auth_repo.rs`, `GeoLookup` port and `MaxmindGeoLookup`; `contracts::DeviceGrantError::Forbidden` added | 29 unit + about 24 integration for the grant, 7 for GeoIP | Expired pending grants are closed as `denied` (no `expired` status); the test mmdb is built at test time by `maxminddb-writer` (no binary committed); no real GeoIP file exists |
| AZ-3870_worker_scratch | Done | `sunnycloud-worker::scratch`: scratch tree with modes and group, sweep, remove on every exit path, `statvfs` input check, safe-open of decoder output (`openat2` with fallback, `O_PATH`, `st_nlink == 1`, cap, reopen through the descriptor) | 14 integration + 17 unit | Needs `/proc`; other-gid cases partly run (supplementary group); `work/` concern still open |
| AZ-3961_sync_hash_file | Done | `sync-core`: `hash_file` (one block buffer, resume from `ResumeHashState`, state handed over every 32 blocks through `CheckpointStore::save`, CPU share throttle, identity check at start and end), `HashedFile`, `HashCheckpoint` | 18 new | AC-5 (1 GiB CPU) replaced by a formula test and a timing test; resume state has no file identity |
| AZ-3965_feed_client | Done | `sync-core`: `poll_changes` (clamped wait, client timeout rule, `check_page` on every page, backoff with jitter and `Retry-After`, 410 and 422 to `ResyncRequired`), `head_cursor`, host-side resync helper in tests | 9 integration + 5 unit | AC-6 (no cursor to a host method) is structural; head answer shape assumed |

## Code Review Verdict: PASS_WITH_WARNINGS

Review of the joined tree: 5 findings, none blocking: a resumed hash state carries no file identity; the pacing test compares wall-clock times; the output reopen needs `/proc`; an expired grant is stored as `denied`; a failing GeoIP lookup logs once per request.

CI-parity: PASS with a local PostgreSQL 18 (`DATABASE_URL`, throwaway container, roles as in earlier batches). In `src/`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo test -p sc_observability` without the `db` feature, the dependency-graph check of the worker and decoder, the four wasm32 builds, `cargo deny check advisories bans licenses sources`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. All 15 steps RC 0.

## Test Suite

- Total: 1473 (workspace, default features, real PostgreSQL 18)
- Passed: 1470
- Failed: 0
- Skipped: 3 (AZ-3892 AC-7 casefold true branch; AZ-3888 AC-7 UI driver; AZ-3961 5 GiB single-buffer test, run by hand and passed)

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | Device grant: `mismatch` means different addresses (not different cities); unknown or consumed device codes answer `ExpiredToken` (no `InvalidGrant`); an unknown platform is an unexpected error (the route layer validates it); `UserCode` has no public constructor from text (E-21 needs one); expired pending grants close as `denied` (an `expired` status needs a migration) | AZ-3687 | Update the 01 description and data model 4.5 | unclear |
| 2 | GeoIP: `geoip_db_path` is an mmdb file (GeoLite2-City compatible), English names, `City, Country` or the country alone; a missing or invalid file means unknown and the service starts; the operator supplies the file; deployment docs need a note; the test fixture DS-32 is built at test time | AZ-3687 AC-7 | Record in helper 01, tests.md and the deployment docs | clear |
| 3 | Scratch: `decoder_gid` and the output caps are now readable by the worker; decoder-side contract: files and directories in `out/` must be group-accessible (mode 0660 for `result.<ext>`, directories group-writable) or the output is rejected as unusable and the sweep cannot clear them | AZ-3870, E-19 | Record in the decoder doc | unclear |
| 4 | Hashing: `ProgressSink` unit is bytes `(hashed, total)`; the state goes through `CheckpointStore::save` as an `UploadCheckpoint` in state `Hashing` (resolves `host_traits.md` open points 2 and 3); `hash_cpu_share` is a percent `u8` while the contract text says 0.25; `ResumeHashState` has no identity | AZ-3961 | Update `host_traits.md` and `sync_api.md`; hosts key saved states to the file identity | unclear |
| 5 | Feed: `GET /changes/head` answers `{"cursor": "E.S"}` (assumed); `wait` is whole seconds on the wire; the hold-cap error (G-05-2) is not named and falls into `Unexpected`; a poll is one request so 429 reaches the caller | AZ-3965 | Pin in `api_v1_surface.md` and the feed contract | unclear |
| 6 | Disk: a worker found the disk full (`src/target` 133 GB, 50 GB of it `incremental`) and deleted the incremental-cache entries older than 3 hours without asking (regenerable, no sources); the disk is at 98% after this gate | process | Owner decides whether to clear `target/debug/incremental` (about 29 GB) or move the target directory | unclear |
| 7 | Carried from earlier batches and still open: provider-linked wire shapes, `EditedSince.current`, audit-retention interval key, `task` log field, mobile sign-in completion, `SessionInfo` fields, sliding refresh expiry, image checks never run | batches 2-14 | See their reports | unclear |

## Commit

`[AZ-3687] [AZ-3870] [AZ-3961] [AZ-3965] device grant with GeoIP, worker scratch, resumable hashing, feed client` — Loop: 1

## Next Batch

Ready now: AZ-3749 quota skeleton and AZ-3723 blobstore skeleton (both add a migration and need the per-test database harness: a shared test-support crate is advisable first), AZ-3871 worker decoder client, AZ-3893 platform fs windows (cannot be compiled here).
