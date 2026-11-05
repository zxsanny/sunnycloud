# Flutter UI for iOS: progress with ETA, coverage banner, settings, help

**Task**: AZ-3980_ios_flutter_ui
**Name**: Flutter UI for iOS
**Description**: The iOS screens show "backing up n of N" with the permission level and an ETA, the limited-access coverage banner, the iCloud opt-in setting and best-effort help text, fed by status from Swift over the EventChannel
**Complexity**: 3 points
**Dependencies**: AZ-3975_ios_uniffi_static_library, AZ-3976_ios_photokit_asset_source, AZ-3999_flutter_backup_ui, AZ-4000_flutter_channel_bridge
**Component**: 18 ios_app [v1.1] (Flutter UI with the Swift channel)
**Tracker**: AZ-3980
**Epic**: AZ-3641 (E-28)
**Ref**: E-28/c08

## Dependencies

### Document Dependencies
- `_docs/02_document/components/17_android_app/description.md` §2 (Flutter UI, `BackupStatus`), channel bridge of E-27/c16.
- `_docs/02_document/components/18_ios_app/description.md` §5 (foreground catch-up, banner, no completion claim).

## Problem

On iOS the reliable path is a foreground catch-up: the user keeps the app open and watches the backlog shrink (AC-9 iOS). The UI must make that visible ("backing up n of N" with an ETA and the permission level), tell the user under limited access how many photos the app can see and how to allow more (AC-53), offer the iCloud "Optimise Storage" opt-in, and never claim that the backlog completes in the background. Status reaches Flutter from Swift over the EventChannel; Flutter never calls Rust and never sees a token.

## Outcome

- In the foreground the user sees "backing up n of N" (N = assets the app can see), the permission level and an ETA, updated while the run progresses.
- Under limited access the banner reads exactly `N photos visible, tap to allow more`, and a tap opens the limited-library picker.
- Settings include the iCloud download opt-in, default off.
- Help text says background backup is best effort with no time guarantee.

## Scope

### Included
- iOS status view: progress count, permission level, ETA, pause reason, skipped counts by reason (same as 17).
- The coverage banner (same wording as 17, no library total) and the tap that opens the limited-library picker through Swift.
- Settings entry for the iCloud opt-in (default off) and its explanation (Wi-Fi and charging).
- Help page with the best-effort wording.
- Test IT-18-3 and the banner leg of IT-18-2.

### Excluded
- Status production, the run and the channel bridge itself (E-27/c06, E-27/c16), the Android screens (E-27/c15).
- The visible-id diff and picks handling (E-28/c04); the iCloud gating logic (E-28/c09).
- The sign-in screen's provider rule (E-28/c06).

## Acceptance Criteria

**AC-1: Foreground progress with ETA (AC-9 iOS)**
Given 1,000 pending assets and the app open in the foreground
When the run progresses
Then the screen shows "backing up n of N" with N equal to the visible assets, the permission level and an ETA, and n rises until the backlog completes while the app stays open

**AC-2: Progress survives a suspend**
Given a foreground run halfway through
When the screen is locked and the app reopened
Then the progress resumes from the checkpoint value (not from 0), and the UI makes no claim that the backlog finished while locked

**AC-3: Coverage banner (AC-53)**
Given "Selected Photos" authorization with 20 photos visible
When the status screen opens
Then the banner reads exactly `20 photos visible, tap to allow more`, shows no library total, and a tap opens the limited-library picker

**AC-4: iCloud opt-in setting**
Given a fresh install
When the settings screen opens
Then the iCloud download opt-in is off, and switching it on is sent to Swift over the channel

**AC-5: No token in Dart**
Given a signed-in app with sentinel tokens
When channel payloads seen by Dart are captured during sign-in and a run
Then 0 payloads contain a token

## Non-Functional Requirements

**Performance**
- Status updates are driven by EventChannel events; the UI does not poll the core.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Banner text for 5 and 20 visible photos | Exact wording with N; no total |
| AC-4 | Default of the opt-in setting | Off |

## Blackbox Tests

Appium XCUITest on DEV-IPHONE against the real server stack (harness E-28/c11a); no device means `NOT-RUN(H-IOS)`.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 1,000 pending (DS-21 subset) | Foreground run | n of N, level, ETA; completes in foreground | IT-18-3, FT-P-31 |
| AC-2 | Run halfway; lock and reopen | Progress | Resumes from checkpoint; no completion claim | IT-18-3 |
| AC-3 | Selected Photos, 20 visible | Banner and tap | Exact text; picker opens | IT-18-2, FT-P-35 |
| AC-4 | Fresh install | Settings | Opt-in off; change reaches Swift | IT-18-8 |
| AC-5 | Sentinel tokens | Channel payloads | 0 tokens | |

## Constraints

- Flutter never calls Rust; Swift owns the UniFFI handle and sends status over the EventChannel (S4-4).
- Same banner wording as 17 (AC-53).

## Risks & Mitigation

**Risk 1: ETA misleads on a suspended app**
- *Risk*: the ETA suggests completion while the screen is locked.
- *Mitigation*: AC-2 checks that no completion claim is made for locked time.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The shared Flutter `lib/` is owned by 17 and changes there go through an E-27 task (module-layout, 18 entry); this task's Dart screens need either an E-27 task or an owner decision giving 18 a Dart area | Owner / E-27 | open | Medium |
| G-7: no time bound is set for the foreground catch-up of a 100-photo backlog (FT-P-31) | Owner | open | Low |
| How the ETA is computed (rate window, what counts) is not stated in 18 | E-28 | open | Low |
