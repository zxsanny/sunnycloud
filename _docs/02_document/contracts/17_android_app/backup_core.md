# Contract: backup_core — `BackupCore`, `AssetSource` and the backup outcome types (crate `mobile_backup`)

**Component**: 17_android_app (crate `mobile_backup`, UniFFI to Kotlin and Swift, Rust crate API for 18)
**Producer task**: E-27/c07 — `_docs/02_tasks/todo/B13-07_backup_core_surface.md` (UniFFI surface, `set_server`, `status`, `cancel`, wire-error mapping); contributors: E-27/c05 (`B13-05`, crate and library form, checkpoint states), E-27/c06 (`B13-06`, `run_backup`, `AssetSource`, `RunContext`, `BackupOutcome`), E-27/c08 (`B13-08`, `exchange_code`, `refresh`, `sign_out`, `CredentialProvider` over `TokenVault`), E-27/c12 (`B13-12`, `ServerCheck.trust`), E-27/c13 (`B13-13`, upload names and folder recovery), E-27/c14 (`B13-14`, `rebuild_checkpoint`, `RebuildReport`), E-27/c06b (`B13-35`, the [v1.1] iOS additions below); keys assigned at Step 4
**Consumer tasks**: E-27/c09 (Kotlin `AssetSource`), c10 (entry points), c11 (first sync), c12, c15, c16 (channel bridge), c18 (binding drift check); E-28/c03 (Swift package, AZ-3975), E-28/c04 (PhotoKit `AssetSource`, AZ-3976), E-28/c05 (Keychain credentials, AZ-3977), E-28/c06..c10 (AZ-3978..AZ-3982); pipelines AZ-3672, AZ-3673, AZ-3674
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

`BackupCore` is the one backup implementation for Android and iOS (ADR-013, AC-52). Kotlin (17) and Swift (18) hold its UniFFI handle; Flutter never calls it. Hosts implement `AssetSource` so the core can list and read photos without touching platform APIs. Hosts and the channel bridge read this file, not the producer tasks.

Sources: `components/17_android_app/description.md` §2, §2a, §4, §5, §10, §15 risk 0; `epics/E-27_android_app.md` § Interface Specification; `contracts/13_sync_core/sync_api.md`, `host_traits.md`, `uniffi_surface.md`; `tests/spec-gaps.md` §1 (`set_server`, `location_redacted`).

## Shape

### `BackupCore` methods

| Method | Input | Output | Errors (`BackupError`) | Blocking? |
|---|---|---|---|---|
| construction | host-supplied app-private directory, client version (the device label is an input of `Backup::new`, built with the session, c08) | handle | none; the checkpoint DB opens lazily on first use | sync |
| `set_server` | `url` | `ServerCheck{trust}` (open point 2 for other fields) | `HttpsRequired`, `ServerUnreachable`, `UpgradeRequired{message}` | async |
| `exchange_code` | `code`, `code_verifier`, device label | `()`; tokens stored through the host credential store | `InvalidCode`, `ServerUnreachable`, `UpgradeRequired{message}` | async |
| `refresh` | none | `()` (single flight in 13) | `NotSignedIn`, `TokenRejected`, `ServerUnreachable` | async |
| `sign_out` | none | `()`; server revoke attempted, then the host store is cleared even if the server is unreachable | none | async |
| `run_backup` | `RunContext{trigger, deadline, network_policy}`, `AssetSource` | `BackupOutcome` | `NotSignedIn`, `ServerUnreachable`, `UpgradeRequired{message}`, `TokenRejected` | async |
| `status` | none | `BackupStatus` | `CheckpointUnavailable` | sync (reads the checkpoint) |
| `rebuild_checkpoint` | listing page callback | `RebuildReport` (open point 3) | `ServerUnreachable` | async |
| `cancel` | none | `()`; sets a flag the run checks between blocks; in-flight 13 operations end `Cancelled` within 1 s | none | sync |

### Types

| Type | Shape |
|---|---|
| `RunContext` | `{trigger, deadline, network_policy}`; `trigger` names the entry point (MediaStore change, catch-up, app open, first sync); `network_policy` carries the Wi-Fi-only setting and the current network state (exact form: open point 4) |
| `BackupOutcome` | `Completed` \| `Partial{remaining}` (deadline or cancel reached) \| `Paused(PauseReason)` \| `Skipped(AlreadyRunning)` |
| `PauseReason` | `QuotaExceeded`, `NoNetwork`, `MeteredNetwork`, `PermissionDenied`, `LimitedAccess`, `ServerRejectedName`, `UpgradeRequired`, `AccountNotActive`, `SignedOut` |
| `BackupStatus` | `{visible, uploaded, pending, paused: Option<PauseReason>, skipped{gone, name, size, failed}, last_error: Option<{message, correlation_id}>, access_level, location_redacted: bool}` |
| `AccessLevel` | `Full` \| `Limited` \| `Denied` |
| `ServerCheck` | `{trust: System \| UserAdded, ...}` |
| `BackupError` | `NotSignedIn`, `ServerUnreachable`, `UpgradeRequired{message}`, `TokenRejected`, `InvalidCode`, `HttpsRequired` |
| `ChangeToken` | opaque bytes; the core stores and returns them, never interprets them |
| `AssetDescriptor` | `{asset_id, change_token, display_name, mime, size, date_taken, relative_path}` |

### `AssetSource` (host callback trait; Kotlin over MediaStore, Swift over PhotoKit)

| Method | Signature (indicative) | Errors | Rule |
|---|---|---|---|
| `assets_after` | `(token: ChangeToken, limit) -> [AssetDescriptor]` | none | visible assets only; pending and trashed rows filtered |
| `current_token` | `() -> ChangeToken` | none | |
| `list_visible_ids` | `(page) -> [asset_id]` | none | every asset visible now, same filters |
| `open_read` | `(asset_id) -> ReadableFd` | `AssetGone` | opens the original (Android: `setRequireOriginal`); reports whether the copy is location-redacted |
| `access_level` | `() -> AccessLevel` | none | read at the start of every run |

### Wire errors to checkpoint states and pauses (applied inside `run_backup`)

| From 13 (`sync_api.md`) or the host | Result | Loop continues? |
|---|---|---|
| `QuotaExceeded`, `FreeSpaceFloor` (507) | `Paused(QuotaExceeded)`; asset stays pending, retried next run | no |
| `AccountNotActive` | `Paused(AccountNotActive)` | no |
| `UpgradeRequired{message}` (426) | `Paused(UpgradeRequired)`; server text in `last_error` | no |
| `SizeTooLarge`; batch entry `size_too_large` | asset `skipped_size` | yes |
| `InvalidName`; batch entry `invalid_name` | asset `skipped_name` | yes |
| host `OpenError::Gone` (host `NotFound`/`PermissionDenied`), or `Io{NotFound\|PermissionDenied}` | asset `skipped_gone` | yes |
| `SourceChanged`, other `Io`, host `OpenError::Failed`, `BlocklistMismatch`, `Unexpected{code, request_id}`, batch entry with any other code | asset `failed`, `last_error` with a correlation id; final until the change token moves | yes |
| batch entry `upload_session_gone`, `parent_not_found`, `parent_deleted` | asset stays pending (next run; c13) | yes |
| `ParentNotFound`, `ParentGone` | `Err` until E-27/c13 recreates the folder | no |
| `Unauthorized` (`TokenRejected`) | `Err(RunError::Sync(Unauthorized))`; the host store is cleared and sign-in shown | no |
| `Transport`, `RateLimited`, `CredentialUnavailable`, `SessionCapReached`, `StagedCapReached`, any other error not listed | `Err(RunError::Sync(..))`, assets stay pending | no |
| host call without an answer (`assets_after`, `current_token`, `list_visible_ids`, `visible_count`, `describe`, `access_level`) | `Err(RunError::Source)`; nothing in the checkpoint changes; the next run retries | no |
| network policy not met | `Paused(NoNetwork)` or `Paused(MeteredNetwork)`, checked first, no call made | no |
| `access_level() = Denied` | `Paused(PermissionDenied)` | no |

Checkpoint asset states: `new, hashing, uploading, uploaded, skipped_gone, skipped_name, skipped_size, failed`.

### Run loop as built (E-27/c06, AZ-3990)

Plain Rust API, no UniFFI export yet (E-27/c07): `Backup::new(api, db, AndroidConfig, device_id, device_label)`, `run_backup(RunContext{trigger, deadline}, Arc<dyn AssetSource>) -> Result<BackupOutcome, RunError>`, `cancel()`. One `Backup` per process holds the run mutex; a concurrent call answers `Skipped(AlreadyRunning)`. `device_id` is a constructor input so two processes with separate checkpoints derive the same `client_key` (who stores it is c07/c08). Additions to `AssetSource` that the loop needs: `visible_count()` and `describe(ids)` (name, size and token of an id); `list_visible_ids(page)` pages from 0 until an empty page; `open_read` answers `AssetGone` for a vanished file. The change token is opaque bytes stored as a BLOB under the `cursor` key `change_token` (also `access_level`, `visible_count`, `camera_folder`); hosts must return strictly increasing tokens and never split equal tokens across a page. `client_meta` is `{"device", "asset_id"}` on both upload paths. Asset states used: `new`, `uploading` (hashed, key stored, not acknowledged), `uploaded`, `skipped_gone`; a changed token resets an asset to `new`. `network_policy` of `RunContext` and the mapping of permanent per-asset errors to `skipped_*` and pauses come with c07.

### `BackupCore` as built (E-27/c07, AZ-3991)

Exported through UniFFI (feature `uniffi`; async methods use `async_runtime = "tokio"`): `BackupCore::new(directory, client_version)`, `async set_server(url) -> ServerCheck`, `status() -> BackupStatus`, `cancel()`. Rust-only: `BackupCore::with_probe(db, client_version, probe)`, `checkpoint() -> Arc<CheckpointDb>`, `attach(Arc<Backup>)`. `run_backup` is not exported until the c08 credential provider exists (open item: "export `run_backup` with a connected-client factory"). Kotlin bindings were generated from the built library with uniffi 0.32.2 (`BackupCore`, `ForeignAssetSource`, `RunContext`, `BackupOutcome`, `BackupStatus`, `ServerCheck` and the enums); the drift check is c18.

- `ServerCheck { trust: ServerTrust }`, `ServerTrust = System | UserAdded`; `trust` is always `System` until c12.
- `set_server`: sync-core's `ServerUrl::parse` refuses `http://` before any socket (unless the debug-only feature `allow-plain-http`), then `connect` checks `GET /api/v1/server-info`. `ConnectError` maps: `InsecureScheme` to `HttpsRequired`; `Unreachable` to `ServerUnreachable`; `UpgradeRequired{message}` to `UpgradeRequired{message}`; `InvalidConfig{server_url}` to `InvalidServerUrl`; `ProtocolMismatch`, `Unauthorized`, other `InvalidConfig`, `TrustStoreUninitialized` to `ServerUnreachable` (cause logged).
- `BackupError = NotSignedIn | ServerUnreachable | UpgradeRequired{message} | TokenRejected | InvalidCode | HttpsRequired | InvalidServerUrl | CheckpointUnavailable` (the last two are new, non-breaking).
- `BackupStatus { visible, uploaded, pending, paused: Option<PauseReason>, skipped: SkippedCounts{gone, name, size, failed}, last_error: Option<LastError{message, correlation_id}>, access_level, location_redacted }`, read from the checkpoint (counts from `asset`; `paused`, `last_error`, `last_error_id`, `access_level`, `location_redacted` are `cursor` keys). `visible` is `count(*)` of `asset`, queried apart from the per-state counts so the sum rule is a real check; `pending` = `new`, `hashing`, `uploading`; `failed` counts under `skipped.failed`. `paused` and `last_error` are cleared when a run starts; `last_error` survives a restart. `correlation_id` is the server request id when the error has one, else a fresh UUIDv7 that is also in the log line.
- `RunContext{trigger, deadline: Option<Duration>, network_policy}`: `deadline` is a budget from the start of the run; `network_policy = {wifi_only, network: Offline|Metered|Unmetered}`.
- `AssetSource` (Rust): every method except `open_read` returns `Result<_, SourceError>`; `open_read` returns `Result<OpenedAsset{file, location_redacted}, OpenError{Gone|Failed}>`. UniFFI foreign twin `ForeignAssetSource` (`with_foreign`): every method answers `Result<_, HostError>`, `open_read` returns `ForeignOpened{file: ForeignFileSource, location_redacted}`, `describe` takes `Vec<i64>`, `ChangeToken` crosses as bytes; `sync_core::Foreign<dyn ForeignAssetSource>` adapts it, and a host exception arrives as `HostError::Failed`, never a panic.
- `Backup::cancel()` (and `BackupCore::cancel()` through `attach`) sets the stop flag and calls `BackupApi::cancel()` only while a run is in progress; a real sync client stays cancelled for good, so each run after a cancel needs a new connected client and a new `Backup`.

Open points: 5 is answered as "`failed` is final until the change token moves"; 7 (`FreeSpaceFloor`) maps to `QuotaExceeded` with its own `last_error` text ("free space floor reached").

### Library form and checkpoint store (E-27/c05, AZ-3989)

`mobile_backup` builds as `lib`, `cdylib` (one `.so` per Android ABI) and `staticlib` (iOS), with the UniFFI scaffolding behind feature `uniffi` (a `library_version()` probe until E-27/c07). The checkpoint database opens on first use (`CheckpointDb::write` and `read`), at schema version 1 (`SCHEMA_VERSION`); `SqliteCheckpointStore` is the `CheckpointStore` of sync-core over its `session` and `batch` tables (`session` keyed by the host's `local_key`; `batch.entries` is the JSON of the member `BatchEntry` list; `load_batches` answers in `commit_id` order, as `MemoryCheckpointStore` does). `AndroidConfig` reads the `android_*` rows of the shared key table (`android_catchup_interval` 15m, `android_unmetered_only` true, `android_charging_only` false, `android_hash_ahead` 8).

## Invariants

- One run per process: a second `run_backup` while one runs returns `Skipped(AlreadyRunning)` and makes no `SyncApi` call (AC-78 layer 2).
- Every upload is idempotent: `client_key = derive_client_key(device_id, asset_id, file_hash)`; a batch `commit_id` is a random UUIDv7 stored before the first staging and never equal to a `client_key` (AC-78 layer 3).
- An asset reaches `uploaded` only after the commit acknowledgement; its content hash is stored before its upload starts (AC-52).
- At every `status` call: `visible = uploaded + pending + skipped.gone + skipped.name + skipped.size + skipped.failed`.
- No access or refresh token crosses this surface in any return value, status field, error text or log line; the core reaches tokens only through the host credential store (`CredentialProvider`), whose `on_rotated` returns only after the pair is stored.
- `set_server` and every later request refuse `http://` unless the build has the debug-only plain-HTTP feature (AC-54); release builds never have it.
- `sign_out` always clears the host store, also when the server is unreachable.
- Camera backup never targets a vault folder (AC-14 camera half).
- Uploads go to `Camera Uploads/<device label>/`; a name taken in that folder becomes `<stem> (<asset_id>)<ext>`.

## Non-Goals

- Not covered: the Flutter channel schema (`status_channel.md`), the upload protocol and token refresh rules (13 contracts), how hosts store tokens or read photos.
- Not covered: v1.2 vault calls (`vault-core`, `contracts/19_vault_core/vault_core_api.md`).

## Versioning Rules

- **Breaking**: a method or type removed or renamed, a signature changed, a `PauseReason` or `BackupError` variant removed, a wire-table row changed.
- **Non-breaking**: a new optional `BackupStatus` field, a new `AssetDescriptor` field with a default, a new error variant hosts already treat as generic.

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| one-run | two `run_backup` calls within 1 ms | one runs; the other `Skipped(AlreadyRunning)` with 0 `SyncApi` calls | IT-17-1 |
| two-processes | two processes, own checkpoints, same 100 assets, real server | 100 server files; identical `client_key` per asset | IT-17-3 |
| plain-http | `set_server("http://...")` in a release-profile build | `HttpsRequired`, 0 sockets | IT-17-6, AC-54 |
| wire-errors | 507, `SizeTooLarge`, `InvalidName`, `AssetGone`, `AccountNotActive`, 426 | the table rows above; counts add up at every `status` | IT-17-15 |
| no-token | every return value and status after a signed-in run with sentinel tokens | 0 sentinel occurrences | ST-17-1 |
| sign-out-offline | `sign_out` with the server unreachable | store cleared; `run_backup` then fails `NotSignedIn` | IT-17-17 |

## Open points

1. The construction inputs (directory, client version, device label, server address persistence) are not stated in 17 §2; E-27/c07 records them here.
2. `ServerCheck` fields other than `trust` (for example the server version) are not stated.
3. `RebuildReport` fields are not stated; E-27/c14 records them.
4. The exact form of `RunContext.trigger` and `network_policy` is not stated.
5. When an asset moves to `failed`, and whether a `failed` asset is retried, is not stated.
6. `PauseReason` values with no stated trigger: `LimitedAccess`, `ServerRejectedName`, `SignedOut` (versus the `NotSignedIn` error).
7. 13's `FreeSpaceFloor` (507) has no `PauseReason`.
8. `sign_out` uses `DELETE /api/v1/devices/me`, which is `DeviceOnly` (`sync_api.md` open point 4); the mobile route needs an owner answer.
9. The meaning and filling rule of the v1.1 fields `group_id` and `content_identifier` is decided by E-28/c02 (AZ-3974); E-27/c06b records it here before the 1.1.0 rows below are frozen.
10. The shared config keys are named `android.*`; 18 §10 expects `mobile.*` when iOS is built.
11. The approved Android mockup and 18 §1 both show a time-left estimate; `BackupStatus` has no field for it (see `status_channel.md` open point 1).

## Planned 1.1.0 additions [v1.1] (producer: E-27/c06b — `_docs/02_tasks/todo/B13-35_ios_shared_crate_additions.md`)

| Item | Change | Rule |
|---|---|---|
| `AssetDescriptor.group_id` | new optional field | links an original and its edited render (and Live Photo parts) per E-28/c02; Android passes none |
| `AssetDescriptor.content_identifier` | new optional field | the PhotoKit identifier chosen by E-28/c02; Android passes none |
| `open_read` error `NeedsDownload` | new host error | the asset stays pending with the reason `needs_download`; it is neither skipped nor retried again in the same run |
| `BackupStatus` pending reason | pending assets waiting on `NeedsDownload` are reported with that reason (field form recorded by E-27/c06b) | they still count in `pending`, so the sum rule holds |
| checkpoint columns | `content_identifier`, `group_id` on `asset`, added by a forward migration | a v1.0 checkpoint opens and migrates with both columns empty |

All 1.1.0 additions are optional for hosts: an Android host that never sets them behaves exactly as in 1.0.0.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B13) |
