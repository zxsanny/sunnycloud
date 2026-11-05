# Contract: status_channel — native-to-Flutter status events and commands

**Component**: 17_android_app (Kotlin side and Dart side of the Method/EventChannel; 18 implements the same channel in Swift)
**Producer task**: E-27/c16 — `_docs/02_tasks/todo/B13-16_flutter_channel_bridge.md`; keys assigned at Step 4
**Consumer tasks**: E-27/c15 (Flutter screens), E-27/c17 (help and diagnostics), E-27/c12 (user-CA warning); E-28/c08 (iOS Flutter UI with the Swift channel, B09-28) and the other E-28 Swift tasks that send status; pipelines AZ-3672, AZ-3673, AZ-3674
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

Flutter only displays state and sends user commands; the native layer (Kotlin on Android, Swift on iOS) owns the `BackupCore` handle (ADR-013, S4-4). This contract fixes what crosses the channel so the shared Dart screens work on both platforms and no token ever reaches Dart.

Sources: `components/17_android_app/description.md` §2 (Flutter UI, `BackupStatus`), §11; `contracts/17_android_app/backup_core.md`; `ui_mockups/android-backup.html` (approved frames); `ui_mockups/DESIGN.md` line "Not drawn".

## Shape

### Event stream `status` (native → Dart)

Each event is a full snapshot, sent on every change and once when Dart subscribes.

| Field | Type | Required | Description | Source |
|---|---|---|---|---|
| `signed_in` | bool | yes | a session is stored | native credential store |
| `server_url` | string | no | https address when set | `set_server` |
| `server_trust` | `system` \| `user_added` | no | `user_added` shows the persistent warning | `ServerCheck.trust` |
| `device_label` | string | no | label chosen at sign-in; folder `Camera Uploads/<device label>/` | sign-in |
| `running` | bool | yes | a run is active | native |
| `paused` | `PauseReason` name or null | yes | | `BackupStatus.paused` |
| `visible`, `uploaded`, `pending` | int | yes | counts the app can see | `BackupStatus` |
| `skipped` | `{gone, name, size, failed}` ints | yes | per reason | `BackupStatus.skipped` |
| `last_error` | `{message, correlation_id}` or null | yes | | `BackupStatus.last_error` |
| `access_level` | `full` \| `limited` \| `denied` | yes | `limited` shows `"{N} photos visible, tap to allow more"` with N = `visible` | `BackupStatus.access_level` |
| `location_redacted` | bool | yes | originals with location could not be read | `BackupStatus.location_redacted` |
| `battery_exempt` | bool | yes | battery optimisation exemption granted | Android power manager |
| `settings` | `{unmetered_only, charging_only, buckets}` | yes | current user settings | native settings store |

### Commands (Dart → native, MethodChannel)

| Command | Arguments | Result | Errors |
|---|---|---|---|
| `setServer` | `url` | `{trust}` | `HttpsRequired`, `ServerUnreachable`, `UpgradeRequired{message}` |
| `signIn` | `device_label` | `()` after the browser round trip | `InvalidCode`, `ServerUnreachable`, `UpgradeRequired{message}`, `Cancelled` |
| `signOut` | none | `()` | none |
| `updateSettings` | `{unmetered_only, charging_only, buckets}` | the stored settings | `Invalid{field}` |
| `startLargeBackup` | none | `()`; native starts the first-sync job with its notification | `NotAllowed{reason}` |
| `requestPhotoAccess` | none | `()`; opens the system picker or the full-access prompt | none |
| `requestBatteryExemption` | none | `()` | none |
| `exportDiagnostics` | none | `()`; native asks the user to confirm before anything leaves the app | `Declined` |

## Invariants

- No field, argument or result is named like or carries an access token, refresh token, one-time code or PKCE verifier (sentinel check, IT-17-19, ST-17-1).
- Dart never calls Rust; every command is handled by native code that calls `BackupCore` or a platform API.
- `visible = uploaded + pending + skipped.gone + skipped.name + skipped.size + skipped.failed` in every event.
- No event carries a library total; under limited access only `visible` is shown (AC-53).
- Error names are the typed names above; the message shown to the user is chosen in Dart from the name, except `UpgradeRequired`, which shows the server text.

## Non-Goals

- Not covered: screen layout and wording beyond the AC-53 banner (E-27/c15, mockup).
- Not covered: `BackupCore` itself (`backup_core.md`).

## Versioning Rules

- **Breaking**: a field or command removed or renamed, a type changed, an error name changed.
- **Non-breaking**: a new optional field or a new command that older Dart code ignores.

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| snapshot-on-subscribe | Dart subscribes after a run | one full event with current counts | |
| scripted-status | uploaded 10, pending 5, paused `QuotaExceeded`, skipped {1, 2, 1, 0}, `last_error` with id `c-81` | every value rendered as sent | IT-17-19 |
| no-token | sign in and a run with sentinel tokens; capture every event and command result | 0 sentinel matches | IT-17-19, ST-17-1 |
| typed-errors | `setServer("http://...")` | `HttpsRequired` | IT-17-6 |
| sum-rule | any event | counts add up | |

## Open points

1. The approved mockup shows values with no `BackupStatus` field: last backup time ("Last backup today at 14:32"), a time-left estimate ("About 25 min left"), the account name, and storage used and total in the quota message ("100 GB of 100 GB"). They are not in this schema; adding them is an E-27 change of `backup_core.md` and this file, or the screens leave them out (owner).
2. Where the user settings are stored (native settings store or the checkpoint DB) is not stated; this contract only fixes that native code owns them.
3. The persistent user-CA warning text is not drawn (`DESIGN.md` "Not drawn" row 10); E-27/c12 writes it.
4. Whether assets in buckets the user did not select count in `visible` is not stated.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B13) |
