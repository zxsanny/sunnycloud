# Contract: host_traits — callbacks the hosts implement for `sync-core`

**Component**: 13_sync_core (crate `sync-core`)
**Producer task**: E-23/c14 — `_docs/02_tasks/todo/B09-15_sync_host_traits_foundation.md`; keys assigned at Step 4
**Consumer tasks**: this batch: E-23/c07, c09a, c09b, c10, c11, c13, c15, c18a; E-28/c05 (Keychain store); other batches: E-24 (14 token store, `PlatformFs` file source, LocalIndex checkpoint store, download sink, progress), E-27 (`mobile_backup`: `TokenVault` bridge, `AssetSource::open_read`, checkpoint DB, progress); resolved at Step 4
**Version**: 1.1.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

The five traits through which `sync-core` reaches host state (credentials, file bytes, download target, checkpoints, progress) without owning any persistent data. They are declared once, in `sync-core` (ADR-015, `contracts/sync-core/crate_skeleton.md`); `contracts` never names them. `MemoryCheckpointStore` is the in-memory store for tests and for hosts without a database.

Sources: `components/13_sync_core/description.md` §2 (host-implemented traits), §2a, §3 (small upload, token lifecycle), §4; `tests/spec-gaps.md` §1 (13_sync_core `on_rotated` default); components 14, 17, 18 ports tables.

## Shape

| Trait | Method | Signature (indicative) | Errors | Notes |
|---|---|---|---|---|
| `CredentialProvider` | `bearer` | `() -> Result<Credential, CredentialUnavailable>` | `CredentialUnavailable` | called per request; the core never keeps the value longer than one request; on `CredentialUnavailable` nothing is sent and the call answers `SyncError::CredentialUnavailable`, never retried, never a refresh or a stop of authentication (AZ-4231) |
| | `on_rotated` | `(access: Secret, refresh: Secret) -> Result<(), PersistFailed>` | `PersistFailed` | returns only after the pair is durably stored; the error type follows the Step 5 gap default (retry once, then the refresh counts as rejected) |
| `FileSource` | `len` | `() -> u64` | `Io` | |
| | `read_at` | `(offset: u64, len: u32) -> Result<bytes, Io>` | `Io` | at most one block per call |
| | `identity` | `() -> (size, mtime_ns)` | `Io` | re-checked before commit; a change is `SourceChanged` |
| `DownloadSink` | `write_verified` | `(offset: u64, bytes) -> Result<(), Io>` | `Io` | receives verified blocks only, in order |
| | `finish` | `(verified: bool) -> Result<(), Io>` | `Io` | `verified = true` only after the whole-file rollup matched |
| `CheckpointStore` | `load`, `save`, `clear` | `(local_key) -> Option<UploadCheckpoint>`; `(local_key, UploadCheckpoint)`; `(local_key)` | `Store` | per-file large-upload checkpoint |
| | `save_batch`, `load_batches`, `clear_batch` | `(BatchRecord)`; `() -> [BatchRecord]`; `(commit_id)` | `Store` | small-file batch records |
| `ProgressSink` | `on_progress` | `(done, total)` | none | must be cheap; counts acknowledged work only |

| Type | Shape |
|---|---|
| `Credential` | `{access: Secret, refresh: Option<Secret>}`; `refresh = None` is a desktop device token (never refreshed) |
| `Secret` | the configuration helper's secret wrapper (`sc_config` `Secret<T>`): `Debug` and `Display` print `***` |
| `UploadCheckpoint` | `{session_id, block_hashes, file_hash, commit_id, state}` (13 §2; `commit_id` here holds the upload's `client_key`) |
| `BatchRecord` | `{commit_id, entries: [{staged_key, parent, name, base_revision?, file_hash}]}` |
| `MemoryCheckpointStore` | thread-safe in-memory `CheckpointStore`; one per test |

All traits are object-safe and `Send + Sync`, so hosts pass `Arc<dyn Trait>` and UniFFI can implement them as foreign callbacks (`uniffi_surface.md`).

## Invariants

- `save_batch` for a batch returns before the first `stage_small` of any member; `clear_batch` is called only after the host has stored the per-entry results (AC-40, FT-N-38).
- No new access token is used for any request before `on_rotated` returned `Ok` (AC-58, NFT-RES-33).
- No method of any trait persists or receives the change-feed cursor: `CheckpointStore` has no cursor method (IT-13-12).
- `DownloadSink` never receives bytes of a block whose hash did not match (AC-7).
- Credentials appear only as `Secret`; no trait method takes or returns a token as plain text, and no log line or error text contains one (ST-13-2).

## Non-Goals

- Not covered: where hosts store anything (14 LocalIndex, 17 checkpoint DB, 18 Keychain), their schemas and migrations.
- Not covered: `SyncApi` itself and `cancel()` (`sync_api.md`).

## Versioning Rules

- **Breaking**: a method added without a default, a signature changed, a field removed from a record type.
- **Non-breaking**: a new optional record field with a default.

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| batch-order | spy store; batch of 300 files | `save_batch` precedes every `stage_small` of the batch | IT-13-7 |
| rotate-before-use | spy provider whose `on_rotated` sleeps 300 ms | no request carries the new access token before `on_rotated` returned | IT-13-9 (b) |
| persist-fails | `on_rotated` fails twice | retried once, then `Unauthorized`, no further requests | spec-gaps §1 |
| no-cursor | `CheckpointStore` surface | no cursor method exists | IT-13-12 |
| secret-print | `Debug`/`Display` of a `Credential` with sentinel values | `***` only | ST-13-2 |
| memory-store | save, load, clear, batches across threads | consistent results | IT-13-7, IT-13-17 |
| credential-unavailable | provider answers `CredentialUnavailable`, then recovers | no request sent and authentication not stopped; the next call carries the token | AZ-4231 AC-5 |

## Open points

1. `UploadCheckpoint.state` values are not listed in 13; E-23/c14 proposes them (for example hashing, uploading, committing) and records them here.
2. Whether hash progress (`ResumeHashState`, checkpoint every `sync.hash_checkpoint_blocks`) is stored through `CheckpointStore::save` or handed to the host another way is not stated; E-23/c09a decides with c14.
3. The unit of `ProgressSink::on_progress(done, total)` (bytes or blocks) is not stated.
4. Closed 2026-11-12 (AZ-4231): a host with no credential (signed out) or an unreadable store answers `CredentialUnavailable`.
5. 13 §2 names the field `commit_id` while §3 calls the same value `client_key`; this contract keeps 13's field name.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B09) |
| 1.1.0 | 2026-11-12 | `bearer()` is fallible (`CredentialUnavailable`); open point 4 closed | AZ-4231 |
