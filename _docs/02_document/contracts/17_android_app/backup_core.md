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
| construction | host-supplied app-private directory, client version, device label (open point 1) | handle | none; the checkpoint DB opens lazily on first use | sync |
| `set_server` | `url` | `ServerCheck{trust}` (open point 2 for other fields) | `HttpsRequired`, `ServerUnreachable`, `UpgradeRequired{message}` | async |
| `exchange_code` | `code`, `code_verifier`, device label | `()`; tokens stored through the host credential store | `InvalidCode`, `ServerUnreachable`, `UpgradeRequired{message}` | async |
| `refresh` | none | `()` (single flight in 13) | `NotSignedIn`, `TokenRejected`, `ServerUnreachable` | async |
| `sign_out` | none | `()`; server revoke attempted, then the host store is cleared even if the server is unreachable | none | async |
| `run_backup` | `RunContext{trigger, deadline, network_policy}`, `AssetSource` | `BackupOutcome` | `NotSignedIn`, `ServerUnreachable`, `UpgradeRequired{message}`, `TokenRejected` | async |
| `status` | none | `BackupStatus` | none | sync (reads the checkpoint) |
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
| `QuotaExceeded` (507) | `Paused(QuotaExceeded)`; retried on the next trigger | no |
| `SizeTooLarge` | asset `skipped_size` | yes |
| `InvalidName` | asset `skipped_name` | yes |
| `AssetGone` (host) | asset `skipped_gone` | yes |
| `AccountNotActive` | `Paused(AccountNotActive)` | no |
| `UpgradeRequired{message}` (426) | `Paused(UpgradeRequired)`, server text kept in `last_error` | no |
| `ParentNotFound`, `ParentGone` | camera folder recreated, the asset retried once (E-27/c13) | yes |
| `StagedCapReached` | paced inside 13 | yes |
| network policy not met | `Paused(NoNetwork)` or `Paused(MeteredNetwork)` | no |
| `access_level() = Denied` | `Paused(PermissionDenied)` | no |
| `TokenRejected` | error returned; the host store is cleared and sign-in shown | no |

Checkpoint asset states: `new, hashing, uploading, uploaded, skipped_gone, skipped_name, skipped_size, failed`.

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
