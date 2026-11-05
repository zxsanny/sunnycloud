# Flutter UI: app shell, sign-in, backup status, settings, permission help and coverage banner

**Task**: AZ-3999_flutter_backup_ui
**Name**: Flutter backup UI
**Description**: The shared Flutter screens of the approved Android mockup: server address and sign-in with the https error, backup status (backing up, up to date, needs attention with the exact AC-53 banner, quota pause, permission denied) and settings, inside an app shell the iOS app extends
**Complexity**: 5 points
**Dependencies**: AZ-3992_android_auth_token_vault, AZ-3993_kotlin_asset_source, AZ-4000_flutter_channel_bridge, AZ-4002_native_library_ci, AZ-4003_android_emulator_harness
**Component**: 17 android_app (Flutter UI in the shared Dart app)
**Tracker**: AZ-3999
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c15

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/17_android_app/status_channel.md` — status events and commands (E-27/c16).
- `_docs/02_document/ui_mockups/android-backup.html` (approved frames 1 to 6) and `ui_mockups/DESIGN.md` (tokens, Flutter theming row).
- `components/17_android_app/description.md` §2 (Flutter UI).

## Problem

Flutter only displays state that Kotlin sends and sends user commands back (ADR-013, S4-4). The screens must show exactly what the core reports: the counts the app can see, pause reasons, skipped assets by reason, the last error with its reference, and under limited access the exact text `"{N} photos visible, tap to allow more"` with no library total (AC-53). The address field must refuse `http://` with a clear message before anything is sent (AC-54). iOS (18, v1.1) reuses these screens and adds its own, so the shell must be shared.

## Outcome

- An app shell with two tabs (Backup and Settings), light and dark themes from the design tokens, and a place where platform-specific screens and settings entries register; 18 (E-28/c08) adds its iOS screens and the iCloud setting there.
- Screens matching the approved frames: sign-in (frame 1, with the https error), backing up (frame 2, with the "Start large backup" card and notification text), up to date (frame 3), needs attention (frame 4: AC-53 banner, quota pause, skipped by reason, last error with reference), photo access denied (frame 5), settings (frame 6: Wi-Fi only, charging only, albums, battery section, account, sign out, help, share diagnostics).
- Settings changes sent to Kotlin and confirmed back; no token or code ever in Dart.

## Scope

### Included
- App shell, navigation, theming and the platform extension point for 18.
- Server address and sign-in screen; the https rule's message; the sign-in button that starts the browser through the channel.
- Status screen states listed above; tap on the banner sends `requestPhotoAccess`.
- Settings screen (the battery and vendor help pages themselves are E-27/c17).
- Tests IT-17-19, AT-17-2 and the screen leg of IT-17-6.

### Excluded
- The channel itself (E-27/c16); battery/OEM help pages and diagnostics export (E-27/c17); the user-CA warning (E-27/c12).
- iOS screens (E-28/c08 adds them in this shell); file browsing (out of v1.0).
- End-to-end acceptance AT-17-1, AT-17-3 and AT-17-4 (pipelines AZ-3672, AZ-3673).

## Acceptance Criteria

**AC-1: Status rendered faithfully (IT-17-19)**
Given a scripted status event with uploaded 10, pending 5, paused `QuotaExceeded`, skipped gone 1, name 2, size 1, failed 0, and `last_error` with reference `c-81`
When the status screen shows it
Then progress, the pause reason with its message, each skipped count with its reason and the last error with `c-81` are shown, and the values add up to `visible`

**AC-2: Exact AC-53 banner (AT-17-2, AC-53)**
Given limited access with 20 of 300 photos shared
When the app opens
Then the banner reads exactly `20 photos visible, tap to allow more`, no library total or "of 300" appears anywhere, a tap opens the system picker, and after picking 10 more the next run's uploads raise the count

**AC-3: Plain-HTTP address refused on screen (IT-17-6 screen leg, AC-54)**
Given the release build on the sign-in screen
When `http://sunnycloud.test:8080` is entered and submitted, then an `https://` address
Then the first shows the message "Use an address that starts with https://. sunnycloud does not sign in over plain HTTP." with 0 cleartext requests in the emulator capture, and the second proceeds to sign-in

**AC-4: Pause and denied states**
Given status events for `Paused(QuotaExceeded)`, `Paused(PermissionDenied)`, `Paused(MeteredNetwork)` and `Paused(UpgradeRequired)` with server text
When each is shown
Then each matches its frame (quota and denied) or shows its reason, the upgrade pause shows the server's text, and "Allow access" on the denied screen sends `requestPhotoAccess`

**AC-5: Settings round trip (IT-17-19)**
Given defaults Wi-Fi only on and charging only off
When the user changes Wi-Fi only, charging only and the album selection
Then each change is sent through `updateSettings`, the screen shows the stored values returned by native code, and the values survive an app restart

**AC-6: No token in Dart (IT-17-19)**
Given sign-in and a run with sentinel tokens
When all channel payloads seen by Dart are captured
Then no field is named like a token and no value matches a sentinel

## Non-Functional Requirements

**Compatibility**
- Light and dark themes from the design tokens; `ThemeMode.system` by default (DESIGN.md).

**Reliability**
- Every failure shown to the user carries its reference (correlation id) when the core gave one; no empty catch in Dart.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Widget tests with scripted status events | All values rendered; sum shown |
| AC-2 | Banner text for N = 1, 20, 412 | Exact text; no total |
| AC-4 | Each pause reason | Matching screen and message |

## Blackbox Tests

Flutter integration tests and Appium on the emulator with the real Kotlin layer (harness E-27/c19).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Scripted status | Status screen | Faithful rendering | IT-17-19 |
| AC-2 | 20 of 300 shared | Open, tap, pick 10 | Exact banner; picks uploaded | AT-17-2, FT-P-35, AC-53 |
| AC-3 | Release build | http then https address | Message, 0 cleartext; proceeds | IT-17-6, FT-N-26, AC-54 |
| AC-4 | Quota full on the server | Run | Quota frame with the message | FT-N-50 |
| AC-5 | Defaults | Change settings, restart | Stored and shown | IT-17-19 |
| AC-6 | Sentinel tokens | Capture Dart payloads | 0 matches | IT-17-19, ST-17-1 |

## Constraints

- Dart never calls Rust; it uses only the channel commands of `status_channel.md`.
- The shared Dart app is owned by 17; 18's iOS screens and settings entries are added in this shell by E-28/c08 (coordinator direction at Step 2), and module-layout's 18 entry ("read-only for 18") must be updated to match.

## Risks & Mitigation

**Risk 1: Mockup values without a status field**
- *Risk*: the frames show last backup time, time left, account name and storage used and total, which `BackupStatus` does not carry.
- *Mitigation*: the screens leave those elements out until the owner adds the fields (Flagged concerns); no value is invented in Dart.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The approved frames show last backup time, a time-left estimate, the account name and storage used/total, none of which is in `BackupStatus` (`status_channel.md` open point 1); 18 §1 also needs the time-left estimate | Owner / E-27 | open | Medium |
| module-layout's 18 entry calls 17's `lib/` read-only for 18, while E-28/c08 adds iOS screens in this shell; the layout entry needs the owner's update | Owner | open | Low |
