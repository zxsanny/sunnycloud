# Contract: contracts part 3 — 04 blobstore, 06 sharing, 08 media jobs, 20 vault ports

**Component**: `contracts` (server-only port crate, E-00)
**Producer task**: E-00/c05 — `_docs/02_tasks/todo/B01-14_contracts_part3.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: other batches: E-10, E-13, E-14, E-16, E-18, E-21, E-22, E-30
**Version**: 1.2.0
**Status**: draft (frozen when the producer task is accepted; `VaultService` stays a v1.2 proposal, 20 §1)
**Last Updated**: 2026-11-11

## Purpose

The remaining server ports: blob storage (04), share links (06), media jobs and the worker-facing service (08) and the vault service (20, v1.2). With this part every port of E-00 § Interface specification exists in `contracts`. The blob value types `BlobRef`, `ByteStream`, `PlaintextStream`, `ByteRange` are declared in part 2 and used here.

Conventions are those of part 1 (`part1_registries_identity_admin_quota.md` § Conventions). Sources: `components/04_blobstore/description.md` §3 and Ports, `epics/E-10_blobstore.md` § Interface specification, `components/06_sharing/description.md` §2 and Ports, `components/08_media_jobs/description.md` §2, §4, §5, `components/20_vault_service/description.md` §2-3, `data_model.md` §6, §9.

## Shape

### 04 blobstore

Errors are the typed enum `BlobError` = `NoSpace`, `FreeSpaceFloor`, `Io`, `BlobMissing`, `KeyUnavailable`, `Corrupt`, `RangeNotSatisfiable`, `OutOfRange`, `SizeMismatch` (E-10), and `KeyError` for the keyring.

| Port | Method | Input -> Output | Errors |
|---|---|---|---|
| `BlobWriter` | `plan` (`sync`, pure) | `account, plaintext_size` -> `BlobPlan{blob_id (v4), wrapped_key, master_key_id}` | `KeyUnavailable` |
| | `create_file` | `BlobPlan, preallocate: bool` -> `BlobHandle` | `NoSpace`, `FreeSpaceFloor`, `Io` |
| | `open_for_write` | `blob_id, wrapped_key, master_key_id, plaintext_size` -> `BlobHandle` | `BlobMissing`, `KeyUnavailable` |
| | `write_block` | `&BlobHandle, block_index, plaintext: Bytes` -> `DurableToken` | `NoSpace`, `Io`, `OutOfRange` |
| | `await_durable` | `DurableToken` -> `()` (after the covering group `fdatasync`) | `Io` |
| | `write_complete` | `account, size, body: ByteStream` -> `WrittenBlob{plan, size}` (no row) | `NoSpace`, `FreeSpaceFloor`, `SizeMismatch`, `Io` |
| | `discard` | `blob_id` -> `()` (missing is Ok) | `Io` |
| `BlobReader` | `open` | `blob_id, wrapped_key, master_key_id, plaintext_size, range: Option<ByteRange>` -> `PlaintextStream` (first chunk verified before return) | `BlobMissing`, `RangeNotSatisfiable`, `Corrupt`, `KeyUnavailable` |
| | `read_head` | same key material, `max_bytes` (<= 64 KiB) -> `Bytes` | as `open` |
| `BlobRegistry` | `register_in_tx` | `tx: &mut DbTx, WrittenOrPlanned{Written(WrittenBlob) \| Planned(BlobPlan)}, state: Uploading \| Committed` -> `()` | `Unexpected` |
| | `mark_committed_in_tx` | `tx: &mut DbTx, blob_id, size` -> `()` | `Unexpected` |
| | `reinstate_in_tx` | `tx: &mut DbTx, blob_id` -> `Reinstated \| BeyondRetention \| FileMissing` | `Unexpected` |
| | `remove_row_in_tx` | `tx: &mut DbTx, blob_id` -> `()` | `Unexpected` |
| `BlobCatalog` | `get_ref` | `account, blob_id` -> `Option<BlobRef>` (a foreign id is `None`) | none |
| `GcQueue` | `schedule_in_tx` | `tx: &mut DbTx, blob_id, GcReason{Replaced \| Deleted \| AccountDeleted \| DerivedRemoved}, Due{AfterRetention \| Now}` -> `()` | `Unexpected` |
| | `pending_for_account` | `account` -> `u64` | none |
| `Scrubber` | `start_run` | `ScrubMode{Full \| Due}, rate: Option<ByteRate>` -> `ScrubRun{started_at}` | `Unexpected` |
| | `run_step` | `&ScrubRun` -> `StepResult{checked, mismatched, missing, done}` | `Unexpected` |
| | `finish_run` | `&ScrubRun` -> `()` | `Unexpected` |
| | `status` | `()` -> `ScrubStatus{oldest_scrubbed_at, unscrubbed, latest_summary, unavailable}` | none |
| `BlobInventory` | `find_missing` | `batch` -> `Vec<Uuid>` (committed blob ids whose file is absent) | `Unexpected` |
| `MasterKeyring` | `load_and_verify` | `()` -> `Keyring` | `KeyError{FileMissing \| WrongLength \| KeyCheckMismatch \| KeyCheckAbsentButBlobsExist}` |
| | `init_key_check` | candidate key -> `()` | `KeyError::KeyCheckSampleFailed{blob_id}` |
| | `rewrap_batch` | `from_id, to_id, n` -> rows rewrapped `u64` | `KeyError` |
| `EpochMarker` | `write` | `epoch: i64` -> `()` (atomic) | `Io` |
| | `read` | `()` -> `Option<i64>` (absent marker is `None`) | `Io` |

### 06 sharing

| Port | Method | Input -> Output | Errors |
|---|---|---|---|
| `ShareService` | `create` | `account, node_id, expires_in?, password?` -> `CreatedShare{id, token (plaintext, only here), url_path, expires_at, has_password}` | `TargetNotFound`, `TargetNotShareable`, `ExpiryOutOfRange` |
| | `list` | `account, node_id?` -> `Vec<ShareInfo>` (no token) | none |
| | `revoke` | `account, share_id` -> `()` (idempotent) | `NotFound` |
| `PublicShareService` | `open` | `token, client_ip` -> `ShareOpen{Available{name, size, media_type, expires_at} \| PasswordRequired \| NotFound}` | `RateLimited{retry_after}` |
| | `verify_password` | `token, password, client_ip` -> `AccessGrant{value, expires_at}` | `NotFound`, `WrongPassword`, `RateLimited{retry_after}` |
| | `authorize_content` | `token, Option<AccessGrant>, client_ip, starts_download: bool` -> `ShareContent{account_id, node_id, revision_id}` | `NotFound`, `PasswordRequired`, `RateLimited{retry_after}` |
| | `authorize_derived` | `token, Option<AccessGrant>, client_ip` -> `ShareDerived{account_id, file: LiveFile}` (see Open points) | `NotFound`, `PasswordRequired`, `RateLimited{retry_after}` |

`NotFound` is one variant for unknown, expired, revoked, tombstoned and purged links (AC-12).

### 08 media jobs

| Port | Method | Input -> Output | Errors |
|---|---|---|---|
| `ThumbnailStateSource` | `states` | `account, &[FileHash]` -> map hash -> `ThumbState{Ready \| Pending \| Failed \| None}` | none |
| `DerivedOpener` | `open_derived` | `account, node_id, DerivedKind` -> `BlobRef` | `NotReady`, `NotFound` |
| | `open_derived_for` | `&LiveFile, DerivedKind` -> `BlobRef` | `NotReady`, `NotFound` |
| `WorkerAuthenticator` | `authenticate` | `bearer: &str` -> `WorkerIdentity` | `Forbidden` (every cause) |
| `WorkerJobs` | `claim` | `Lane{Image \| Video}, worker_id` -> `Option<ClaimedJob>` | `Unexpected` |
| | `extend_lease` | `job_id, worker_id` -> `Extended{lease_ms}` | `LeaseLost` |
| | `release` | `job_id, worker_id` -> `()` | `LeaseLost` |
| | `complete_skipped` | `job_id, worker_id, SkipOutcome{SkippedCompliant \| Obsolete}` -> `()` | `LeaseLost` |
| | `fail` | `job_id, worker_id, code, Disposition{Retry \| Permanent}` -> `FailResult{Requeued \| Dead}` | `LeaseLost` |
| | `open_input` | `job_id, worker_id` -> `PlaintextStream` (exact length known) | `LeaseLost`, `RevisionObsolete` |
| | `accept_output` | `job_id, worker_id, DerivedKind, media_type, width?, height?, duration_ms?, body: ByteStream, length` -> `Accepted \| AlreadyPresent` | `LeaseLost`, `RevisionObsolete`, `OutputTooLarge`, `OutputTypeRejected`, `OutputKindRejected`, `Storage` |
| | `record_status` | `worker_id, WorkerStatus{decoder_ok, image_age_days}` -> `()` | `Unexpected` |
| `MediaAdmin` | `requeue_dead` | `filter{account?, kind?}` -> count `u64` | `Unexpected` |
| | `stats` | `()` -> `MediaStats` (fields open) | `Unexpected` |

| DTO | Fields / variants |
|---|---|
| `ClaimedJob` | `job_id, kind: JobKind{Stills \| Poster \| Transcode}, content_hash: FileHash, params_version, attempt, lease_ms` |
| `DerivedKind` | `Thumb`, `Preview`, `Transcode` |
| `WorkerIdentity` | the authenticated worker (no account) |

### 20 vault service (v1.2 proposal)

| Port | Method | Input -> Output | Errors |
|---|---|---|---|
| `VaultService` | `create_vault` | `account, header: bytes, root_node_id` -> `(vault_id, ETag)` | `VaultExists`, `HeaderTooLarge`, `NotFound` |
| | `get_header` | `account, vault_id` -> `(header, ETag)` | `NotFound` |
| | `replace_header` | `account, vault_id, header, if_match: ETag` -> `ETag` | `NotFound`, `HeaderTooLarge`, `PreconditionFailed` |
| | `report_unlock_failure` | `account, vault_id` -> `()` (best effort) | none |

## Invariants

- 04 is reached only through these ports; `BlobCatalog::get_ref` is the only request-path read of `blobs`, and `BlobReader` is the one decrypt path (downloads and the worker input alike).
- `GcQueue::schedule_in_tx` is the single writer of `blob_gc` and of `blobs.state = gc_pending`.
- `WorkerAuthenticator` fails with one variant for every cause; the worker routes are never reachable with a user or device credential (AC-79).
- `PublicShareService` returns one `NotFound` variant for every missing, expired or revoked link (AC-12).
- No port here returns plaintext of a vault file or any key material other than wrapped keys.
- All conventions of part 1 hold.

## Non-Goals

- Not covered: file format, AEAD, group commit, sweeps, the job planner, lease SQL, Argon2id, vault cryptography.
- Not covered: the HTTP routes of the worker listener (08 §5 through 11) and their JSON (see Open points 2).

## Versioning Rules

- As parts 1 and 2. `VaultService` may change before v1.2 without a major bump while its status is "proposal"; it gets a frozen 1.x when E-30 starts.

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| listing-complete | public traits after part 3 vs every port name in E-00 § Interface specification | 0 missing (E-00 AC 3) | the listing test of parts 1-2 becomes complete |
| fake-per-port | a recording double of each part-3 trait | compiles; holdable as `Arc<dyn Port>` | |
| worker-auth-uniform | match on `WorkerAuthenticator` error type | exactly one variant | AC-79 |
| share-notfound-uniform | match on `PublicShareService::open` result | one `NotFound` variant, no reason field | AC-12 |
| no-client-edge | crate graph | no client crate, `media-shared` or worker/decoder crate imports `contracts` | ADR-015 |

## Open points (flagged in the producer task)

1. `PublicShareService::authorize_derived` is named by 11 §2 and 08 §4 (it returns the `LiveFile` that `open_derived_for` takes, with the account that owns it as `ShareDerived`) but is missing from 06's own method table; its inputs here mirror `authorize_content`.
2. Worker job DTOs (`ClaimedJob`, `WorkerStatus`, `Lane`, outcomes) are also declared in `media-shared` for the supervisor (09). `contracts` may not import `media-shared` (both layer 2) and 11 may not import it either, so the same wire shape exists twice. Default here: declared in `contracts` for the API side, with identical field names and serde names; a wire test in E-18/E-20 keeps them equal. Alternative for the owner: allow `contracts` (or 11) to import `media-shared`.
3. 12 lists `GcSweeper::run_cycle` for `admin gc-run`; E-00 does not name `GcSweeper` as a port. This contract treats it as the 04 `Janitor` implementation that 12 picks by name.
4. `MediaStats` fields and `ScrubStatus.latest_summary` shape are not specified.
5. `BlobHealth::probe` (04 §3) is used for `/readyz` and metrics through a `ReadinessCheck` implementation, not a port of its own.
6. 08's derived blobs have a media type, but `BlobRef` carries none; 11 must pass it to `DownloadService::open_blob_content` (part 2 Open point 1).

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B01) |
| 1.1.0 | 2026-11-11 | `PublicShareService::authorize_derived` returns `ShareDerived{account_id, file}` instead of a bare `LiveFile`, so 11 can pass the owning account to 03 for the derived blob; owner-approved; 06 does not implement it yet, the callers are 11 (registered share thumbnail route) and the test doubles | owner-approved change |
| 1.2.0 | 2026-11-13 | `DerivedBlob{blob, media_type}` returned by `DerivedOpener`; `open_derived_for` takes `account` (parent-approved, AZ-3844); `CreatedShare` `Debug` hides `url_path` | AZ-3844, AZ-3697 |
