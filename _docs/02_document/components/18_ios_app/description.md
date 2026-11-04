# 18_ios_app [v1.1, stub-level]

## 1. High-Level Overview

**Purpose**: The iOS client: sign in and back up the photo library on a best-effort basis, with the same upload implementation as Android.

**Single responsibility**: iOS entry points (PhotoKit, background scheduling, Keychain, sign-in) around the shared Rust crate `mobile_backup` defined in 17_android_app. It adds no protocol code.

**Architectural pattern**: same three layers as 17: Flutter UI, Swift platform layer, Rust core through UniFFI (Swift) in one static library. Swift owns the UniFFI handle and sends status to Flutter over a Method/EventChannel; there is no flutter_rust_bridge.

**Upstream dependencies**: 17_android_app's `mobile_backup` crate (shared), 13_sync_core, 11_http_api, helpers 01, 02, 05, 06; 19_vault_core (v1.2).
**Downstream consumers**: the user.

### Scope
In (v1.1): Sign in with Apple plus other providers *only when Sign in with Apple is enabled on the server* (App Store guideline 4.8), system-browser/one-time-code sign-in (`ASWebAuthenticationSession`), PhotoKit `AssetSource`, authorization states (authorized, limited, denied), foreground catch-up with visible progress and ETA, system-scheduled background work (`BGProcessingTask`, `BGAppRefreshTask`), Keychain token (this-device-only accessibility, not synced to iCloud), coverage banner, iCloud "Optimise Storage" download opt-in (default off, Wi-Fi and charging).
Out (v1.1): the iOS 26.1+ background upload extension and its 104-response endpoint (deferred; `mobile_backup` stays sans-IO so it can be added), Live Photo pairing details beyond the linked-resource upload (FT-P-33 is deferred and written when v1.1 is planned), file browsing, vault camera backup (never).

## 2. Internal Interfaces

`BackupCore` and `AssetSource` as in 17. Differences:

| Item | iOS behaviour |
|------|---------------|
| `AssetSource.assets_after` | PhotoKit fetch by an opaque `ChangeToken(bytes)` that wraps `PHPersistentChangeToken`; original and edited renders are separate resources linked by a `group_id`. `list_visible_ids` is diffed against the `asset` table when the authorization level or visible count changes, as in 17 |
| `AssetSource.access_level` | `Full`, `Limited` (shows the limited-library picker), `Denied` |
| `open_read` | file URL of an exported resource; iCloud-only assets raise `NeedsDownload`, downloaded only when the opt-in is on |
| Triggers | app open, foreground observer, `BGProcessingTask`, `BGAppRefreshTask` (best effort, no guarantee) |
| Token store | Keychain, `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`; holds the access and the rotating refresh token, persisted before the new access token is used (`CredentialProvider::on_rotated`, as in 17) |

## 2a. Ports

Same format as 13 (client-side traits live in `sync-core`; `contracts` is server-only). `AssetSource` (PhotoKit) and the Keychain token store are Swift-side callbacks inside this component.

| Direction | Port | Defined in | Counterpart |
|---|---|---|---|
| Requires | `BackupCore` | `mobile_backup` (shared crate owned by 17; FFI boundary) | 17_android_app |
| Requires | `SyncApi` | `sync-core` (client crate) | 13_sync_core (through `mobile_backup`) |
| Provides | `CredentialProvider` (`bearer`, `on_rotated`, over the Keychain), `FileSource` (over exported resources), `CheckpointStore` (same schema as 17), `ProgressSink` | `sync-core` | consumed by 13 |

## 3. External API
None. Universal link or custom scheme for the sign-in return.

## 4. Data
Same checkpoint schema as 17 (including `change_token`, `client_key` and the `batch` table) plus `content_identifier` and `group_id` columns (edited/Live pairs); same rebuild-from-server rule after reinstall (listing of the device's camera folder, then batched `lookup_revisions`; no cross-device dedupe). Server folder `Camera Uploads/<device label>/` and the name-collision suffix from the asset id are as in 17.

## 5. Implementation Details
- The Rust loop runs only while the process runs (foreground, or inside a granted background task window of about 30 s to minutes); every run is resumable from the checkpoint, so a suspended app loses at most one block.
- Foreground catch-up shows progress, an ETA and "backing up n of N" (N = assets the app can see) with the permission level; under limited access the coverage banner reads "N photos visible, tap to allow more" (same wording as 17, AC-53); no claim that the backlog completes (screen lock suspends the app).
- The sans-IO design of the core is kept so system-owned background `URLSession` uploads (file-based only) or the iOS 26.1 extension can drive it later.
- The UI hides Google and GitHub sign-in unless the server reports Sign in with Apple enabled (providers list from the API).

## 6. Helpers
As 17.

## 7. Caveats
iOS limits background work; camera backup is best effort. iCloud-optimised assets may need a large download first. Limited authorization and permission changes can occur at any time and are re-read at the start of each run.

## 8. Dependency Graph
Must be implemented after: 17 (`mobile_backup`), Sign in with Apple on the server (01, v1.1). Parallel with: 14 macOS work. Blocks: nothing.

## 9. Logging and security
As 17 (no tokens or file names in logs; Keychain only; plain HTTP refused).

## 10. Configuration
Shared mobile keys from 17 (`mobile.*` names to be unified with `android.*` when 18 is planned).

## 11. Acceptance criteria implemented
AC-9 (iOS: best effort, foreground catch-up), AC-52, AC-53, AC-54, AC-19 (iOS provider rule), AC-46 (iOS reinstall) [v1.1]. **Interaction ACs folded in**: none owned (AC-78 is owned by 17 through `mobile_backup`; the same in-process mutex plus idempotent `client_key` design applies on iOS, and a v1.1 sibling criterion is proposed).

## 12. Test scenarios
FT-P-31, FT-P-35, FT-P-63, FT-N-34, FT-N-26, NFT-RES-27, NFT-RES-28, NFT-RES-LIM-03, NFT-SEC-44 (Keychain check). FT-P-33 deferred.

## 13. Component acceptance
Foreground catch-up shows visible resumable progress; background runs best effort; Google/GitHub sign-in hidden without Sign in with Apple; reinstall uploads 0 media bytes; limited access shows the coverage banner.

## 14. Risks
1. Background upload constraints (static `BackgroundUploadURLBase`, 104 interim responses unsupported by hyper) keep the extension out of v1.1.
2. Needs Apple Developer account, signing and App Store review; guideline 4.8 wording may change.
3. Depends on the Android device spike results for the shared library layout.
