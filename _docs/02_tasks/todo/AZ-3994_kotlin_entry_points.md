# Kotlin entry points: `BackupWorker`, `MediaTriggerWorker`, `CatchUpWorker`, `AppOpenKick`

**Task**: AZ-3994_kotlin_entry_points
**Name**: Kotlin entry points
**Description**: Start every automatic backup through one unique WorkManager job `backup` that calls `run_backup`: a MediaStore content-URI trigger re-armed after each run, a 15-minute catch-up, and an app-open kick that never starts a second loop, with the user's network and charging settings as constraints
**Complexity**: 5 points
**Dependencies**: AZ-3991_backup_core_surface, AZ-3993_kotlin_asset_source, AZ-4000_flutter_channel_bridge, AZ-4002_native_library_ci, AZ-4003_android_emulator_harness
**Component**: 17 android_app (Kotlin platform layer)
**Tracker**: AZ-3994
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c10

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/17_android_app/backup_core.md` — `run_backup`, `RunContext`, `BackupOutcome` (E-27/c06, c07).
- Records of E-27/c02 (Play policy), E-27/c04 (OEM delays, catch-up interval).

## Problem

Backup must start from the system, not from the Flutter UI: a WorkManager worker runs without a Flutter engine, and must start uploading within 10 s of a trigger (AC-52). OEM task killers drop triggers, so a periodic catch-up must heal missed ones (R32). The foreground app and the background worker must not run two loops (AC-78 layer 1): the app-open kick enqueues the same unique work and only raises a "rerun after" flag.

## Outcome

- `BackupWorker` as the unique work `backup` (policy keep), calling `run_backup` once per execution and reporting the outcome.
- `MediaTriggerWorker` on the MediaStore images and video URIs, re-armed after each run.
- `CatchUpWorker` every `android.catchup_interval` (15 minutes) with the catch-up trigger, so the visible-id diff runs.
- `AppOpenKick` that enqueues `backup` and sets the "rerun after" flag; a run that ends with the flag set runs once more.
- Constraints from settings: unmetered network by default, optional charging only; the library loaded once per process, no Flutter engine needed.

## Scope

### Included
- The four entry points above and their `RunContext` (trigger, deadline from the WorkManager execution window, network policy).
- Unique-work policy keep; the rerun flag.
- Constraint updates when the user changes settings (via E-27/c16).
- Tests IT-17-2, the emulator leg of IT-17-9 and IT-17-18.

### Excluded
- The user-started large first sync (E-27/c11).
- The loop itself (E-27/c06) and the asset source (E-27/c09).
- The status display (E-27/c15, c16).

## Acceptance Criteria

**AC-1: Foreground and background give one file per asset (IT-17-2, AC-78 layer 1)**
Given 100 new photos in MediaStore
When `AppOpenKick` starts and `BackupWorker` is enqueued within 100 ms of each other, repeated for 10 rounds with fresh photos
Then each round leaves exactly 100 server files with 0 duplicates and 0 conflict copies, WorkManager shows one running `backup` work while the second request is kept out, and the rerun after the flag finds 0 new assets and sends 0 bytes

**AC-2: A missed trigger is healed (IT-17-9 emulator leg)**
Given `MediaTriggerWorker` disabled and 5 new photos
When WorkManager time is advanced by `android.catchup_interval` through the test driver
Then the catch-up run uploads the 5 photos with the catch-up trigger, and the stored change token advances

**AC-3: Background start without Flutter (IT-17-18, AC-52)**
Given the app not running and one new 5 MB photo
When the trigger fires
Then the first upload byte reaches the proxy within 10 s, no Flutter engine is started (process list and logcat), the library is loaded once and the database opened on first use, and memory during a following 2 GiB upload stays at most 150 MB (functional on the emulator; numbers from H-ANDROID)

**AC-4: Constraints follow settings**
Given Wi-Fi only on (default) and a metered network, then Wi-Fi only off; and charging only on with the device unplugged
When a trigger fires in each state
Then the work waits on the metered network while Wi-Fi only is on and runs once it is off, and with charging only on it waits until the device charges

**AC-5: Trigger re-armed**
Given a completed run
When a new photo is added afterwards
Then `MediaTriggerWorker` fires again and the photo is uploaded without opening the app

## Non-Functional Requirements

**Performance**
- First byte within 10 s of a background trigger; memory at most 150 MB (AC-52).

**Reliability**
- No always-on foreground service for automatic backup (Play policy, 17 §5).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Unique-work policy and rerun flag with WorkManager's test driver | One running; flag causes one rerun |
| AC-4 | Constraint mapping from settings | Unmetered and charging constraints as set |

## Blackbox Tests

Appium and `adb` on the API 34+ emulator against the real server (harness E-27/c19); WorkManager time through the test driver.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 100 photos per round, 10 rounds | App open and worker within 100 ms | 100 files per round; 0 duplicates | IT-17-2, FT-P-76, AC-78 |
| AC-2 | Trigger disabled, 5 photos | Advance catch-up interval | 5 uploaded | IT-17-9, AC-9 |
| AC-3 | App not running | Trigger, then 2 GiB upload | First byte at most 10 s; no Flutter engine; at most 150 MB | IT-17-18, AC-52 |
| AC-4 | Metered network; unplugged | Trigger | Waits; runs when allowed | |

## Constraints

- Every entry point calls `run_backup` only; none uploads (AC-52; checked by E-27/c11 IT-17-16 and AZ-3673 AC-6).

## Runtime Completeness

- **Capability**: AndroidX WorkManager scheduling (content-URI trigger, periodic work, unique work).
- **Production code that must exist**: the four entry points used in release builds.
- **Allowed stubs**: WorkManager's test driver for time in tests.
- **Unacceptable substitutes**: an always-on foreground service, an alarm-only loop, a loop started from the Flutter UI.

## Risks & Mitigation

**Risk 1: OEM drops the trigger**
- *Risk*: Xiaomi or Samsung kill the trigger.
- *Mitigation*: AC-2's catch-up; the vendor help of E-27/c17.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| How Kotlin reads `android.catchup_interval` and the other `android.*` values is not stated (see E-27/c05) | E-27 owner | open | Low |
