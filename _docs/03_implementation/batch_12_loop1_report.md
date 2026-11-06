# Batch Report

**Batch**: 12
**Tasks**: AZ-3683_authenticator_scopes_cache, AZ-3770_contracts_part3, AZ-3816_admin_crate_skeleton, AZ-3963_small_upload_batch
**Date**: 2026-11-06

Four parallel workers on disjoint crates (`components/01_identity`, `contracts`, `components/07_admin`, `sync-core`). Parent edit: one line in `scripts/ci/run-system-allowlist.txt` (`src/components/01_identity/src/auth_repo.rs`), added on the owner's explicit approval in this session (the authenticator reads a token hash in system scope because the account is unknown until the hash resolves). Shared files touched: `Cargo.lock`, the crates' own manifests, `sync-core` test support (`Script::respond`).

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3683_authenticator_scopes_cache | Done | `sunnycloud-identity`: `IdentityAuthenticator` (both ports), `auth_repo` (4 statements), `session_cache`, `scope`; test harness helpers | 7 unit + 42 integration (all green) | Timing targets of PT-01-1 not run (reference hardware); allowlist line approved by the owner |
| AZ-3770_contracts_part3 | Done | `contracts`: 17 ports (04: 9, 06: 2, 08: 5, 20: 1) with DTOs and errors, listing test completed | contracts tests and 4 doctests green | Many shapes the contract does not fix (see below) |
| AZ-3816_admin_crate_skeleton | Done | `sunnycloud-admin`: settings, error-to-code mapping, constructors, grep gate | 19 | No wire code for `ConfirmationMismatch` / `InvalidLimit`; two settings without a default; constructors narrower than the contract text |
| AZ-3963_small_upload_batch | **Partly done** | `sync-core`: batch executor (save before stage, pacing, chunking, replay, `batch_commit`), `upload_small`, `resume_batches`, `Script::respond` in the fake | 22 new (129+ in the crate) | `stage_small` is not a public `SyncApi` method: the `Staged` DTO is missing from `sync-core-proto`. Spec stays in `todo/`, ticket stays `In Progress` |

## Code Review Verdict: PASS_WITH_WARNINGS

Review of the joined tree: 8 findings, none blocking: `stage_small` missing (no `Staged` DTO); the batch entry wire shape disagrees with part 2; after a cap hit the saved record is shrunk and the deferred members exist only in memory; `authenticate` serves a cached principal after `revalidate` saw the credential dead; `MasterKey` bytes are not zeroed; worker DTOs duplicated between `contracts` and `media-shared`; admin constructors narrower than the contract; admin settings with an undefined `None`.

CI-parity: PASS with a local PostgreSQL 18 (`DATABASE_URL`, throwaway container, roles as in earlier batches). In `src/`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo test -p sc_observability` without the `db` feature, the dependency-graph check of the worker and decoder, the four wasm32 builds, `cargo deny check advisories bans licenses sources`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. All 15 steps RC 0.

## Test Suite

- Total: 1181 (workspace, default features, real PostgreSQL 18)
- Passed: 1179
- Failed: 0
- Skipped: 2 (AZ-3892 AC-7 casefold true branch, needs `SC_CASEFOLD_DIR`; AZ-3888 AC-7 UI driver)

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `Staged{staged_key, size}` does not exist in `sync-core-proto` or `sync_core_proto.md` (it exists only in the server-side `contracts`, `size: i64`); `stage_small` is `pub(crate)` until it does | AZ-3963 | Owner approves adding the DTO to the proto crate and doc; then `stage_small` is a one-liner | unclear |
| 2 | Batch entry wire shape: proto `BatchEntry` has `parent` and `file_hash`; part 2 says `{staged_key, parent_id, name, base_revision?, client_mtime?, client_meta?, import_hint}` | AZ-3963, E-21/c02 | Server and client owners settle one shape | unclear |
| 3 | Assumed shapes: `PUT /api/v1/staged-uploads/{key}` with `X-Blake3` (block hash) and `application/octet-stream`; `POST /api/v1/commits:batch` answering a `BatchResult` of the same length | AZ-3963 | Pin in `api_v1_surface.md` | unclear |
| 4 | `CheckpointStore::save_batch` must replace a record with the same `commit_id` (the executor shrinks a record after a cap hit); a restage after a kill that follows a successful commit leaves orphaned staged rows on the server | AZ-3963, host_traits | Host stores 14/17 must replace; 03 owner decides whether a staged PUT for a committed key is a no-op | unclear |
| 5 | Authenticator: cached principals live at most `session_cache_ttl` and can outlive `access_expires_at` and a role change by that bound; `RawToken.kind` is ignored; `Scope` has no `DeviceOnly`; hash lookup in system scope (allowlist line added) | AZ-3683, authenticator.md | Update the contract draft | unclear |
| 6 | Contract part 3: `Disposition` of the worker is `FailDisposition` in `contracts`; `LeaseExtended` (not `Extended`); `OutputMeta`, `AcceptOutcome`, `WorkerInput{length, body}`; opaque shapes for `DurableToken`, `BlobHandle`, `Keyring`, `MasterKey`, `ByteRate`, `ScrubSummary`, `ShareInfo`; `VaultService::report_unlock_failure` returns `Result` (404 for unknown vault); `contracts` has no serde, so wire copies are tested not shared | AZ-3770 | Align `part3_blob_share_media_vault.md` | unclear |
| 7 | Admin: `ConfirmationMismatch` and `InvalidLimit` have no wire code (mapped to `None`); `health_check_timeout` and `admin_list_page_size` have no default (kept `Option`); no "cleanup list" is defined for 07; `AdminError` has no `Forbidden` variant; structs are `AdminServiceImpl` etc.; constructors take less than the contract lists | AZ-3816 | E-02 / E-17 owners | unclear |
| 8 | Carried from earlier batches and still open: tree body shapes and `EditedSince.current`, provider list format, AC-20 numbers, audit-retention interval key, `task` log field, Retry-After cap, `bearer()` blocking, image checks never run | batches 2-11 | See their reports | unclear |

## Commit

`[AZ-3683] [AZ-3770] [AZ-3816] [AZ-3963] authenticator, contracts part 3, admin skeleton, small upload batch` — Loop: 1

## Next Batch

Ready now: AZ-3749 quota skeleton (adds a migration; a shared test-database harness is advisable first), AZ-3684 provider sign-in callbacks, AZ-3964 verified download, AZ-3965 feed client, AZ-3966 token lifecycle, AZ-3961 sync hash file (all `sync-core`, one at a time), AZ-3870 and AZ-3871 worker scratch and decoder client, AZ-3893 platform fs windows; AZ-3963 completes once `Staged` is decided.

## Follow-up: AZ-3963 completed (owner approved the `Staged` DTO)

`Staged{staged_key: Uuid, size: u64}` is added to `sync-core-proto` (`transfer.rs`, exported, round-trip test, row in `sync_core_proto.md`). `stage_small(staged_key, bytes, hash) -> Staged` is now a public `SyncApi` method of `HttpSyncClient`; its answer body is decoded, and the batch executor refuses a staged row whose key or size is not the one it sent (`ProtocolViolation`). The fake server answers with the echoed key and size. 4 new tests in `tests/small_stage_one.rs`. The server-side `contracts::Staged` keeps `size: i64`; the doc row says so. The `Staged` JSON shape (`{staged_key, size}`) is still an assumption until E-21 pins it.

Also fixed: the worker test `ac4_healthcheck_...` (and `ac2`, `ac3`, `sigterm_...`) failed in about 1 of 3 gate runs and in about 6% of runs under a light load. Cause: `Running::start` treated a bare TCP connect as "the worker is up", but a connect can succeed on a port that is not yet the worker's, so the next request was refused or reset. `start` now waits for a 200 on `/healthz`, checks that the child has not exited, takes a lock around choosing a port, and the busy-port test takes the same lock; a failing test prints what the worker wrote. 0 failures in 300 runs under the load that gave 6% before.

Gate: all 15 steps RC 0; 1184 passed, 0 failed, 2 ignored. AZ-3963 is `In Testing`; the batch 12 commit table above lists it as partly done, this supersedes it.

Commit: `[AZ-3963] Staged DTO, public stage_small, stable worker test start` — Loop: 1
