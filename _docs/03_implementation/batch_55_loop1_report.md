# Batch Report

**Batch**: 55
**Tasks**: AZ-3716_tree_revision_lookups (done), AZ-3734_transfer_start_upload (done), AZ-3697_share_create_list_revoke (done), AZ-3844_thumbnail_states_derived_opener (done); new follow-up AZ-4264; the free-space floor change (owner decision) by the parent
**Date**: 2026-11-13

Four parallel workers on disjoint crates (`02_tree`, `03_transfer`, `06_sharing`, `08_media_jobs`). Two workers found their crate without the runtime dependencies the task needs (03 and 06 were skeletons); the parent added them mid-batch, plus test-only dependencies for 03, 08 and the identity-style real-component tests. Every lockfile change was path, feature or edge entries; no new external package. The parent wrote one migration (owner-approved), one shared-type fix and the free-space floor change, approved three contract changes, wrote every document change, reviewed, ran the gate and committed.

## Owner decisions 2026-11-13

1. AZ-3734 as specified: 03 takes `Arc<dyn AccountGuard>`; every open large-path session counts toward the cap; a zero-byte file never takes the large path.
2. Free-space floor = the smaller of 10 GiB and 5% of the data volume.
3. `resolve_path` also finds a tombstoned last part, with its deleted state, for restore.
4. The committed-key record of lost-commit recovery is a migration column: `upload_sessions.committed_revision_id`.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3716_tree_revision_lookups | Done | `get_revision`, `is_current_or_retained`, batched `lookup_revisions` (≤1000, one statement on `revisions_account_id_content_hash_idx`), `describe_blobs` (one recursive system-scope statement; a tombstoned file's blob maps to `[]`), `find_revisions` by device (`revisions_client_device_idx`), `resolve_path` (+ inherent `resolve_path_with_state` for restore), `LiveUsageSource::live_bytes`. `impl TreeReader for PgTreeReader` and `impl TreeService for PgTreeService` now exist. AC-1..AC-5 and AC-73 (1,000 nested blobs described within 200 ms) on PostgreSQL 18 with plan checks under the API role |
| AZ-3734_transfer_start_upload | Done | `PgUploadService::start_upload`: size cap, request sanity, name rules, parent check, the key row (committed key within its TTL → `NothingToSend{committed_revision_id}`, open row resumed, other size `ClientKeyMismatch`), the tree `NothingToSend` cases, plan, one transaction (guard, stale committed row deleted, quota reserve, open-session cap, blob and session rows), file after COMMIT with a compensating transaction. AC-1..AC-8 and AC-6b on the real 02 reader, 04 writer and registry, 07a ledger (16 concurrent starts against a cap of 8 open exactly 8) |
| AZ-3697_share_create_list_revoke | Done | `PgShareService`: create (`sc_shr_` token, 7-day default, expiry 1 h..365 d, folder `TargetNotShareable`, every gone target the same `TargetNotFound`), list without tokens (revoked links hidden), idempotent revoke; only the token hash and an Argon2id PHC string stored; `share_created`/`share_revoked` written in the link's transaction, fail closed. AC-1..AC-7 on the real tree reader |
| AZ-3844_thumbnail_states_derived_opener | Done | `ThumbnailStates::states` in two indexed probes (`derived_pkey` index-only, `jobs_key` bitmap; checked at 100,000 rows per table); `DerivedFiles` opens a node's or a live file's derived blob, never the original. IT-08-13 runs on the real 02 publish and planner effect, `PgTreeReader`, 04 catalog and writer/reader, and reads a real WebP back |

## Parent changes

- **Migration** `20261113100000_add_upload_session_commit`: `upload_sessions.committed_revision_id uuid NULL`, composite FK to `revisions (id, account_id)` `ON DELETE CASCADE` (first written as `SET NULL`; changed on the worker's finding that a purged revision would make a committed row look open). Up, down and up checked on a copy of the test template (the CI migration script needs a local `psql`, not installed).
- **Free-space floor** (owner decision): new key `blob_free_reserve_percent` (5, 0..100), `blob_free_reserve` default 20 GiB → 10 GiB; one `free_floor(cap, percent, total)` used by the writer, the health probe and the doctor check; `VolumeFacts` gains `total_bytes`; test configs set the percent to 100 so their explicit reserves keep their meaning; unit test of the min rule.
- **`CreatedShare` `Debug`** no longer prints `url_path` (it carries the token); test in `contracts/tests/token_debug.rs`.
- **Contract changes approved by the parent**: `DerivedOpener::open_derived_for(account, &LiveFile, kind)` (the derived lookup is account-keyed; without the account the share thumbnail answered 500) and `DerivedBlob{blob, media_type}` returned by both opener methods (11 now passes the stored type instead of a fixed `image/webp`/`video/mp4`).
- **Dependencies**: 03 runtime `sc_clock`, `sc_db`, `sc_names`, `sc_observability`, `sqlx`, dev 02/04/07a, `sc_audit`, `bytes`; 06 runtime `sc_audit`, `sc_clock`, `sc_db`, `sc_error`, `sc_ids`, `argon2 =0.6.0`, `getrandom`, `async-trait`, `sqlx`, `thiserror`, `tracing`, `uuid`, dev 02; 08 dev 04, `sc_audit`, `bytes`, `futures-util`.

## Contract and document changes (parent)

`tree_reader.md` 1.2.0, `tree_service.md` 1.1.0; `upload_service.md` 1.3.0, `commit_service.md` (committed-key record), `data_model.md` §6.3 (`committed_revision_id`); `derived_access.md` 1.1.0, part 3 1.2.0, `api_v1_surface.md` open point 26 and `content_serving_headers.md` open point 3 closed; 06 description (audit fail closed, token, `share_argon2_params` default); `blob-writer.md`, `metrics-and-logs.md`, `epoch-marker-and-health.md`, `01_helper_config.md` (free-space floor); AZ-3738 spec gains the commit rule for the committed-key record.

## Code Review Verdict: PASS_WITH_WARNINGS

CI-parity (`CARGO_INCREMENTAL=0`): fmt, workspace clippy `-D warnings`, workspace tests on PostgreSQL 18 (3245 passed, 0 failed, 21 ignored), wasm build and the four check scripts all exit 0.

Fixed during the batch: the share token leaking through `CreatedShare`'s `Debug`; the committed-key FK (`SET NULL` → `CASCADE`); the share thumbnail's missing account and the derived type. Warnings (recorded):
- `contracts::BlobRegistry` has no account and 04 does not implement it as a trait; `FileBlobWriter::plan` keeps refused plans in memory for good → AZ-4264.
- `find_revisions` pages by offset (the port has no cursor); keyset paging needs a contracts change (`tree_service.md` open item).
- `lookup_revisions` and `live_bytes` count a file under a tombstoned folder until the cascade marks it.
- The 03 session cap cannot tell a staged row from a 1-block large session whose block arrived (no row-kind column).
- 03 answers `Unexpected` for `declared_size <= 0` and a bad `client_key`: 11 must refuse both as `invalid_request`.
- The free-space floor is checked in 04's `create_file`, after 03's transaction; a refusal is compensated (nothing reserved).
- Argon2 runs inline on the async worker at share create (fine at the default cost; the visitor `verify_password` path must reconsider).
- Token prefixes: share tokens carry `sc_shr_`; identity tokens carry none, while `data_model.md` says tokens have a type prefix.
- No production wiring yet for `DerivedOpener`, `ThumbnailStateSource`, `PgUploadService`, `PgShareService` (12).

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | 03 and 06 were skeletons without the runtime dependencies their first service needs | AZ-3734, AZ-3697 | Parent added them mid-batch | clear |
| 2 | `describe_blobs` maps a tombstoned file's blob to `[]` (proposed default) | AZ-3716 | Recorded in `tree_reader.md` | unclear |
| 3 | `list` hides revoked links (`ShareInfo` cannot show a revocation); an empty password is hashed like any other | AZ-3697 | As implemented | unclear |
| 4 | After a params-version bump, `open` answers `NotReady` until the new job finishes (older rows are not served) | AZ-3844 | Per `derived_access.md` | unclear |
| 5 | The keyring must start before any `blobs` row exists in a test database | AZ-3844 | Test ordering | clear |
| 6 | The `upload_sessions_active` gauge is only increased by start; commit, abort and reap must decrease it | AZ-3734 | AZ-3738 notes | clear |

## Commit

`[AZ-3716] [AZ-3734] [AZ-3697] [AZ-3844] Tree lookups, upload start, shares, thumbnails` — Loop: 1

## Next Batch

Recompute the ready tasks; AZ-3738 (commit) can now use the committed-key record; AZ-4264 before 12 wires 03.
