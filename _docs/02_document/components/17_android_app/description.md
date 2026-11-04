# 17_android_app

## 1. High-Level Overview

**Purpose**: The Android client: signs the phone in, shows backup state, and backs up photos and videos in the background through one shared Rust uploader.

**Single responsibility**: Android entry points and UI around a single upload implementation. It contains no second copy of the upload protocol: Kotlin triggers and describes assets, Rust uploads, Flutter displays.

**Architectural pattern**: three thin layers over one library. (1) Flutter UI shows status that Kotlin sends over a Method/EventChannel. (2) Kotlin platform layer (WorkManager workers, MediaStore, permissions, token storage, TLS bridge) is the only code that talks to Android APIs and the only owner of the UniFFI handle to Rust; Flutter never calls Rust. (3) Rust crate `mobile_backup` (owned here, reused by 18; this component owns AC-78 through it) holds the backup run, checkpoint DB and asset-to-node mapping, and calls 13_sync_core for hashing, lanes, resume, commits and token refresh. One cdylib per ABI exposes it through UniFFI (Kotlin). There is no flutter_rust_bridge.

**Upstream dependencies**: 13_sync_core, 11_http_api (runtime), helpers 01, 02, 05 (`client_key`, hashing), 06 (names); 19_vault_core (v1.2, same library).
**Downstream consumers**: the user; 18_ios_app reuses `mobile_backup`.

### Scope
In: sign-in through the system browser with a one-time code (PKCE), 15-minute access token with rotating refresh token, HTTPS-only server address, permission states, MediaStore trigger plus periodic catch-up plus app-open plus user-initiated data-transfer job, checkpoint DB, edited-photo linking, reinstall recovery, quota/network pauses, battery-exemption guidance, token storage excluded from backups, status UI and settings. minSdk 30.
Out: the upload protocol (13), file browsing and download on Android (out of v1.0 by owner decision), media rules (compliance bound, remux, poster frame are server side: the app uploads originals unchanged and never transcodes or remuxes), vault (v1.2, camera backup never writes into it), Live Photo pairing (v1.1), iOS code (18), sharing UI (web), cross-device content dedupe (out of scope).

## 2. Internal Interfaces

### Interface: BackupCore (Rust `mobile_backup`; UniFFI to Kotlin)

| Method | Input | Output | Async | Error Types |
|--------|-------|--------|-------|-------------|
| `exchange_code` | `code`, `code_verifier`, device label | `()`; calls 13 `exchange_mobile_code`, stores the tokens through `TokenVault` | Yes | `InvalidCode`, `ServerUnreachable`, `UpgradeRequired` |
| `refresh` | none | `()`; calls 13 `refresh` (single-flight); normally automatic on a 401, exposed so Kotlin can refresh before a run after a long idle | Yes | `NotSignedIn`, `TokenRejected`, `ServerUnreachable` |
| `sign_out` | none | `()`; calls 13 `sign_out` (`DELETE /api/v1/devices/me`), then clears `TokenVault`; the vault is cleared even if the server is unreachable | Yes | none |
| `run_backup` | `RunContext { trigger, deadline, network_policy }`, `Arc<dyn AssetSource>` | `BackupOutcome::{Completed, Partial{remaining}, Paused(PauseReason), Skipped(AlreadyRunning)}` | Yes | `NotSignedIn`, `ServerUnreachable`, `UpgradeRequired`, `TokenRejected` |
| `set_server` | `url` | `ServerCheck`; validates scheme https (debug-only flag allows http), reachability and `GET /api/v1/server-info` (AC-54) | Yes | `HttpsRequired`, `ServerUnreachable`, `UpgradeRequired` |
| `status` | none | `BackupStatus { visible, uploaded, pending, paused, skipped { gone, name, size, failed }, last_error, access_level, location_redacted: bool }` | No (reads checkpoint) | none |
| `rebuild_checkpoint` | listing page callback | `RebuildReport` | Yes | `ServerUnreachable` |
| `cancel` | none | sets a flag the loop checks between blocks | No | none |

`PauseReason` = `QuotaExceeded`, `NoNetwork`, `MeteredNetwork`, `PermissionDenied`, `LimitedAccess`, `ServerRejectedName`, `UpgradeRequired`, `AccountNotActive`, `SignedOut`.

Wire errors from 13 map to checkpoint states and pauses: `QuotaExceeded` (HTTP 507) pauses; `SizeTooLarge` marks the asset `skipped_size`; `InvalidName` marks it `skipped_name`; `AccountNotActive` pauses; `ParentNotFound`/`ParentGone` recreates the camera folder and retries once; `StagedCapReached` is paced inside 13; `ResyncRequired` does not apply (the app uses no feed).

### Interface: AssetSource (implemented in Kotlin, passed to Rust as a callback trait)
- `assets_after(token: ChangeToken, limit) -> Vec<AssetDescriptor { asset_id, change_token, display_name, mime, size, date_taken, relative_path }>`: visible assets only under limited access; rows with `IS_PENDING = 1` or `IS_TRASHED = 1` are filtered out. `ChangeToken(bytes)` is opaque to Rust (Android encodes the MediaStore generation, iOS the `PHPersistentChangeToken`); Rust only stores and returns it.
- `current_token() -> ChangeToken`.
- `list_visible_ids(page) -> asset ids`: every asset the app can see now (same filters). A token cannot reveal assets that became visible later (limited-access picks of older photos), so `run_backup` diffs this list against the `asset` table whenever `access_level` or the visible count changed since the last run, and on every catch-up run; unknown ids are queued as `new`.
- `open_read(asset_id) -> ReadableFd` (raises `AssetGone`): opens the original with `MediaStore.setRequireOriginal(uri)` so redacted Exif location is not applied; this needs the `ACCESS_MEDIA_LOCATION` permission, requested together with the photo permission (without it the original is still uploaded, only the location-redacted copy is available and the app says so in status).
- `access_level() -> AccessLevel::{Full, Limited, Denied}`.

### Interface: Kotlin entry points (all call `run_backup`; none implements upload)
`BackupWorker` (CoroutineWorker, unique work name `backup`); `MediaTriggerWorker` (WorkManager content-URI trigger on `MediaStore.Images/Video` URIs, re-armed after each run); `CatchUpWorker` (periodic, `android.catchup_interval` 15m, queries MediaStore by change token and runs the visible-id diff, so a missed trigger is healed); `FirstSyncJob` (large first syncs: on Android 14+ a JobScheduler user-initiated data-transfer job with notification; on Android 11 to 13 a foreground service of type dataSync started by the user's tap, with the same notification); `AppOpenKick` (foreground; enqueues `backup` as unique work, never runs a second loop). Constraints from settings: unmetered network by default, optional charging only.

### Interface: Flutter UI (Dart; receives `BackupStatus` over an EventChannel from Kotlin, sends commands over a MethodChannel)
Screens: server address + sign-in; backup status (progress, pause reason, coverage banner, skipped counts with reasons, last error); settings (Wi-Fi only, charging only, which buckets); permission help; battery/OEM help (links per vendor: Xiaomi autostart, Samsung, Huawei); about. Text for limited access: `"{N} photos visible, tap to allow more"` (tap opens the system photo picker); no library total is shown because Android 14 hides it. Every asset the app skips is counted and listed by reason (gone, name rejected, too large, failed) so the visible count and the uploaded count always add up with the skipped count.

### Interface: TokenVault (Kotlin)
`save(access, refresh, expires_at)`, `load()`, `clear()`. Storage: Android Keystore-wrapped value in `noBackupFilesDir`; manifest `allowBackup="false"` and data-extraction/backup rules excluding the store and the checkpoint auth fields (FT-N-36). A restored app has no token and shows sign-in. Rust sees it as `CredentialProvider`: `bearer()` returns the stored pair, `on_rotated(access, refresh)` returns only after `save` has completed (see section 5).

### Interface: AuthReturn (Kotlin Activity)
Receives `sunnycloud://auth?code=...&state=...` (custom scheme, or an App Link where configured), passes code and PKCE verifier to `BackupCore.exchange_code`, then clears them. Refuses an address with `http://` unless the debug-only build flag is on (AC-54).

### Interface: TlsTrustBridge
Provides the Android trust store and user-added CAs to rustls through `rustls-platform-verifier`'s Kotlin component, so private-CA self-hosted servers work (owner decision at Step 5: user-added CAs are accepted). When the server certificate chains to a non-system CA, `ServerCheck.trust` is `UserAdded` and the app shows a persistent warning on the server screen. Initialised before the first request in every process (worker or UI).

## 2a. Ports

Same format as 13 (client-side traits live in `sync-core`; `contracts` is server-only). `AssetSource`, `TokenVault` and `TlsTrustBridge` are Kotlin-side callbacks inside this component, not cross-component ports.

| Direction | Port | Defined in | Counterpart |
|---|---|---|---|
| Provides | `BackupCore` (section 2; UniFFI to Kotlin, Rust crate API to 18) | `mobile_backup` (shared crate; FFI boundary) | Kotlin layer of 17, and 18_ios_app |
| Requires | `SyncApi` | `sync-core` (client crate) | 13_sync_core (`HttpSyncClient`) |
| Provides | `CredentialProvider` (`bearer`, `on_rotated`, over `TokenVault`) | `sync-core` | consumed by 13 |
| Provides | `FileSource` (over `AssetSource::open_read`) | `sync-core` | consumed by 13 |
| Provides | `CheckpointStore` (over the checkpoint DB, incl. batch records) | `sync-core` | consumed by 13 |
| Provides | `ProgressSink` | `sync-core` | consumed by 13 |
| Not used in v1.0 | `DownloadSink` | `sync-core` | no downloads on Android |

## 3. External API
None served. Deep links only (`AuthReturn`).

## 4. Data Access Patterns

**Owned data**: checkpoint DB (SQLite, app-private, WAL, `mobile_backup` is the only writer; Kotlin reads through the core). Outside architecture section 4.

| Table | Content |
|-------|---------|
| `asset (asset_id PK, change_token, size, date_taken, content_hash NULL, upload_name NULL, client_key NULL, node_id NULL, revision_id NULL, state, linked_from NULL)` | state: `new, hashing, uploading, uploaded, skipped_gone, skipped_name, skipped_size, failed` |
| `cursor (key, value)` | last `ChangeToken` per volume, access level and visible count seen, device id, camera folder node id |
| `session (asset_id, session_id, block_bitmap_hint)` | resume pointer for an interrupted large upload |
| `batch (commit_id PK, entries)` | a small-file batch: random UUIDv7 `commit_id` plus the member `staged_key`s (= asset `client_key`s), written before the first `stage_small` of the batch and deleted after the results are stored |

Hash saved after hashing, so 10 forced kills re-hash at most 1% of 50,000 assets (AC-52). Rows only move to `uploaded` after the commit ack; a kill between ack and write is healed by the idempotent upload (below). Estimates: 50,000 assets, about 150 bytes each, under 10 MB.

**Server layout and names (F-10)**: assets go to `Camera Uploads/<device label>/` (device label chosen at sign-in; folders created if missing through 13 `create_folder`, the node id kept in `cursor`). `upload_name` is the MediaStore display name; when another asset in the checkpoint (or, after a reinstall, the rebuilt listing) already uses that name in the folder, the name becomes `<stem> (<asset_id>)<ext>`. The server's own conflict-copy naming is only a last resort, not the plan.

**Reinstall (AC-46)**: the checkpoint is rebuilt from the server listing of the device's camera-backup folder (per-revision `client_meta`: device, MediaStore asset id, link to original), falling back to hashing local assets and batched `lookup_revisions` calls (1,000 hashes per request); an asset whose hash is found in this device's folder is linked with 0 bytes uploaded. An asset with the same asset id and a new hash uploads as a new file with `linked_from` set; a new asset id is a new unlinked file. Content already stored by another device is not looked for: there is no commit-by-hash.

## 5. Implementation Details

**One run at a time (AC-78, interaction risk R11)**: three layers, none relied on alone. (1) Kotlin enqueues only the unique work `backup`; a second request while one runs is `KEEP` (and `AppOpenKick` raises a "rerun after" flag). (2) `run_backup` takes an in-process Rust mutex (one per process; Android runs the app in one process, and the mutex is released when the run ends or the process dies, so there is no lease to expire or renew); a concurrent call returns `Skipped(AlreadyRunning)`. (3) Every upload is idempotent: the per-asset `client_key` is `derive_client_key(device_id, asset_id, file_hash)` (helper 05; the same asset and hash always give the same key, never a batch id), so a repeated `start_upload` returns the same session or `NothingToSend{revision}`, and a repeated batch gets the stored response (`commit_id` is random and stored in `batch`). Foreground and background therefore create exactly one file even if layers 1 and 2 were bypassed.

**Token lifecycle (F-01)**: the access token lives 15 minutes, the refresh token rotates. A 401 inside a run is refreshed by 13 (single-flight, one refresh per process), the call is repeated, and a block PUT or a batch is not lost. `on_rotated(access, refresh)` is implemented over `TokenVault.save` and returns only after the new pair is stored (Keystore-wrapped write); only then does 13 use the new access token. If the process dies between the server's rotation and the write, the old refresh token is still stored and works for the server's reuse grace (30 s); after that the server revokes the family and the app shows sign-in (accepted risk, section 15). `TokenRejected` (refresh refused) clears the vault and shows sign-in. The Flutter layer never sees either token.

**Loop**: read the `ChangeToken` cursor; ask `AssetSource.assets_after`; run the visible-id diff when `access_level` or the visible count changed, and on catch-up runs; per asset hash with BLAKE3 (saved), upload through 13 (staged single request up to 2 MiB, batch commit for many small assets, otherwise blocks on 4 lanes; `NothingToSend` ends an already-uploaded asset at 0 bytes), update the checkpoint after the ack. Memory at most 150 MB: block buffers (4 x 2 MiB), no whole-file reads. Start within 10 s of a background trigger: the library is loaded once, the DB opened lazily, no Flutter engine is needed.

**Permission states (AC-53)**: `Full` (READ_MEDIA_IMAGES/VIDEO), `Limited` (Android 14 `READ_MEDIA_VISUAL_USER_SELECTED`, shown as "N photos visible, tap to allow more"; the tap opens the picker or offers full access), `Denied` (backup paused with explanation). Access level and visible count are re-read at the start of every run; photos newly picked under limited access are found by the visible-id diff (their change token may be below the cursor). "Select photos" grants can expire; the app asks for full access and checks again.

**Network and quota**: no Wi-Fi/metered per setting means the work waits (WorkManager constraints); `QuotaExceeded` (507) pauses with a visible message and a retry on the next trigger; an asset deleted between listing and upload is `skipped_gone`; a server name rejection is `skipped_name`; `SizeTooLarge` is `skipped_size`; none of these stops the loop and each is counted in `status.skipped`.

**Battery/OEM**: the app requests the battery exemption, shows vendor-specific help for Xiaomi/Huawei/Samsung, and relies on the 15-minute catch-up instead of the trigger alone. An always-on foreground service for automatic backup is not used (Play policy); the large-first-sync job or foreground service is started by the user and shows a notification.

**Native library**: one `.so` per ABI containing `mobile_backup` + 13 (+ 19 in v1.2) with UniFFI exports; built with NDK r28+ for 16 KB page alignment (CI alignment check), JNA at least 5.17.0. Cancellation is manual via `cancel()`.

**Key dependencies**: Flutter, UniFFI 0.32.2, AndroidX WorkManager, rustls-platform-verifier, rusqlite (bundled), `photo_manager` is not used for background work.

**Error handling**: typed `BackupOutcome` and `PauseReason`; transient network errors back off inside the run; `TokenRejected` clears the token and shows sign-in. No empty catch in Kotlin or Dart; failures reach the status screen with a correlation id.

## 6. Helpers
05_helper_ids_hashing (`client_key` derivation, hashing) through the core; 06_helper_names for display names; 02_helper_error for API errors parsed in Rust.

## 7. Caveats and Edge Cases
- OEM task killers can defer WorkManager until the next app open; catch-up heals it, but AC-9's 60 s is measured only on Pixel; Samsung/Xiaomi p95 at most 5 min with exemption, published per device.
- Android 15 limits dataSync foreground services to 6 h per 24 h; a 150 GB first sync uses the user-initiated job on 14+ (the 11 to 13 fallback has no such cap).
- Two edits of one asset within one change-token tick produce one upload of the latest hash.
- RAW+JPEG are two MediaStore rows and two files; Motion Photo is one file.
- Clock changes do not matter: ordering uses the change token and server order.

## 8. Dependency Graph
Must be implemented after: 13_sync_core (core API), 11 OpenAPI, helpers 01, 02, 05, 06. Parallel with: 14, 15, 16. Blocks: 18_ios_app (reuses `mobile_backup`), 19 integration (v1.2).

## 9. Logging
Rust side logs through a bounded ring buffer file in app-private storage and Logcat in debug; INFO: run start/end counts, pause reasons; WARN: skipped assets, permission changes; ERROR: token rejected, DB error. No file names, no tokens; asset ids only at DEBUG. A "share diagnostics" action exports the ring buffer after user confirmation.

## 10. Configuration
`android.catchup_interval` 15m; `android.unmetered_only` true; `android.charging_only` false; `android.batch_commit_max` (server `batch_max_entries`); `android.hash_ahead` 8 assets. Release-fixed: HTTPS required, minSdk 30.

## 11. Security notes
Tokens only in `TokenVault`, excluded from backup, never in Dart, logs or intents. The Flutter layer never sees the access token or the refresh token. Sign-in: system browser, PKCE, one-time code through the custom scheme; `state` checked; the code is single use (AC-76 on the server). Plain-HTTP refused. Deep-link input is validated before use. Camera backup never targets a vault (AC-14).

## 12. Acceptance criteria implemented
AC-9, AC-46, AC-52, AC-53, AC-54 [v1.0 Android]; RESTRICT-33 token exclusion [v1.0]; AC-14 camera-backup half [v1.2].
**Interaction AC folded in**: **AC-78** [v1.0 Android] — when the foreground app and the background worker upload the same asset at once, exactly one server file is created (mechanism in section 5; test FT-P-76: start `AppOpenKick` and `BackupWorker` simultaneously over a 100-asset fixture, assert 100 server files).

## 13. Test scenarios
FT-P-29, FT-P-30, FT-P-32, FT-P-34, FT-P-35, FT-N-26, FT-N-36, NFT-PERF-12, NFT-PERF-22, NFT-PERF-23, NFT-RES-27, NFT-RES-28, NFT-RES-33 (15-minute token fixture), NFT-RES-LIM-03, NFT-SEC-12, NFT-SEC-44, FT-P-76 (AC-78).

## 14. Component acceptance
- Pixel: new photo on the server within 60 s (battery exempt, Wi-Fi); Samsung/Xiaomi p95 at most 5 min, published; no-phone runs recorded as NOT-RUN(HW-DEVICE).
- 50,000-asset first sync survives 10 forced kills: at most 1% re-hashed, 0 duplicate server files, memory at most 150 MB, background start within 10 s.
- A run longer than 15 minutes against the 15-minute-token fixture completes with one refresh per expiry and no lost block.
- Reinstall with 5,000 backed-up assets: 0 media bytes sent.
- Limited-access picks of older photos are backed up on the next run; every skipped asset is counted with a reason.
- Backup archive contains 0 token occurrences; restored app is signed out.
- Plain-HTTP address is refused; 0 cleartext requests carry tokens or codes.

## 15. Risks
0. Default chosen at Step 5 gap pass: `BackupCore` gains `set_server(url) -> ServerCheck` (https scheme, reachability and `server-info`, AC-54) and `BackupStatus` gains `location_redacted: bool`.
1. Single-library UniFFI build, 16 KB alignment and forced-kill behaviour need the Android device spike.
2. Play Store permission declarations for photo/video access, `ACCESS_MEDIA_LOCATION` and user-initiated transfer limits are not yet verified.
3. Boundary: `mobile_backup` and its checkpoint DB belong to this component; 13 must not duplicate them.
4. A kill between the server's refresh rotation and the vault write, followed by a restart later than the 30 s reuse grace, forces a new sign-in; the persist-before-use order keeps the window to a lost response.
