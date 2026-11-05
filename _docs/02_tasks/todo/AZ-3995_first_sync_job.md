# `FirstSyncJob` and the large first sync (user-initiated transfer, dataSync service, AC-52 runs)

**Task**: AZ-3995_first_sync_job
**Name**: First sync job and AC-52 runs
**Description**: Let the user start a large first backup that keeps running with a notification (a user-initiated data-transfer job on Android 14+, a user-started dataSync foreground service on Android 11 to 13), and prove on the emulator that a 50,000-asset first sync survives 10 forced kills within the memory and throughput limits
**Complexity**: 5 points
**Dependencies**: AZ-3990_run_backup_loop, AZ-3994_kotlin_entry_points, AZ-4002_native_library_ci, AZ-4003_android_emulator_harness
**Component**: 17 android_app (Kotlin platform layer)
**Tracker**: AZ-3995
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c11

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/17_android_app/backup_core.md` — `run_backup` with the first-sync trigger.
- Record of E-27/c02 (user-initiated job and dataSync declarations).

## Problem

A first backup of a whole library (up to 150 GB) cannot finish in WorkManager windows, and Android 15 limits dataSync foreground services to 6 hours per 24 hours, so Android 14+ needs a user-initiated data-transfer job and Android 11 to 13 a user-started foreground service, both with a notification (ADR-016, 17 §7). AC-52's numbers (50,000 assets, 10 forced kills, at most 1% re-hashed, 0 duplicates, at most 150 MB) are first-sync numbers that only an emulator run of the whole app can show. The row was planned at 3 points; it also carries the AC-52 runs IT-17-13, PT-17-2 and PT-17-3, which no other row owns, so it is re-estimated at 5.

## Outcome

- `FirstSyncJob`: started only by the user's tap ("Start large backup"), a user-initiated data-transfer job with notification on Android 14+, a dataSync foreground service with the same notification on Android 11 to 13; both call `run_backup` with the first-sync trigger.
- No always-on foreground service for automatic backup.
- A static check that Kotlin and Dart sources contain no upload-protocol call and no second uploader.
- The AC-52 emulator runs recorded: kills, re-hash share, duplicates, memory and throughput.

## Scope

### Included
- The job and the service, their notifications ("Backing up n of N"), and the start command from the status screen (through E-27/c16).
- The source scan for upload-protocol calls (`/api/v1/uploads`, `/staged-uploads`, `commits:batch`) in Kotlin and Dart.
- Tests IT-17-16, IT-17-13, PT-17-2, PT-17-3.

### Excluded
- The loop and checkpoint (E-27/c05, c06); the automatic entry points (E-27/c10).
- The 16 KB alignment CI check itself (E-27/c18; IT-17-16 reads its result).

## Acceptance Criteria

**AC-1: Right host mechanism per Android version (IT-17-16)**
Given 5,000 new photos on an API 34 emulator and on API 30 and 33 emulators
When the user taps "Start large backup"
Then API 34 runs a user-initiated data-transfer job with a notification, API 30 and 33 run a dataSync foreground service started by the tap with the same notification, and no foreground service runs for automatic backup at any time

**AC-2: One uploader in the sources (IT-17-16, AC-52)**
Given the Kotlin and Dart sources
When the source scan runs
Then it finds 0 upload-protocol calls and 0 second uploader, and a seeded upload call in Kotlin makes the scan fail

**AC-3: 50,000 assets survive 10 forced kills (IT-17-13, AC-52)**
Given DS-21 scaled to 50,000 assets and a rooted emulator
When the first sync runs and the process is killed with `kill -9` 10 times at random moments
Then at most 1% of assets are hashed again, the server holds exactly 50,000 files with 0 duplicates, interrupted large uploads resume from their `session` rows, and no asset is `uploaded` without a server revision

**AC-4: Memory during the first sync (PT-17-2)**
Given the AC-3 run
When memory is sampled every 10 s
Then the peak is at most 150 MB (target 120 MB) and the trend over the run is below 10 MB per hour

**AC-5: First-sync throughput (PT-17-3)**
Given 50,000 assets of 100 KB to 1 MB over the `shape-wan` link and `hash_ahead` 8
When the first sync runs
Then throughput is at least 50% of the link goodput (target 80%) and checkpoint transactions stay at or below 1,000 per second (target 200)

## Non-Functional Requirements

**Performance**
- Memory at most 150 MB; throughput targets of PT-17-3 (proposed by 17's test spec).

**Reliability**
- A kill never creates a duplicate or loses an acknowledged upload.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Source scan on the real tree and on a seeded upload call | Pass, then fail |

## Blackbox Tests

Appium and `adb` on API 34, 33 and 30 emulators against the real server (harness E-27/c19: media push, rooted kill loop, memory sampler).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 5,000 photos on API 34, 33, 30 | Tap "Start large backup" | Job or service with notification | IT-17-16 |
| AC-3 | 50,000 assets, rooted emulator | 10 kills | At most 1% re-hashed; 50,000 files; 0 duplicates | IT-17-13, FT-P-34, NFT-RES-27 |
| AC-4 | AC-3 run | Memory samples | Peak at most 150 MB | PT-17-2, NFT-RES-LIM-03 |
| AC-5 | `shape-wan` link | First sync | At least 50% of goodput | PT-17-3, NFT-PERF-23 |

## Constraints

- The job is started only by the user and always shows a notification (Play policy, E-27/c02 record).

## Runtime Completeness

- **Capability**: JobScheduler user-initiated data-transfer job (Android 14+) and dataSync foreground service (Android 11 to 13).
- **Production code that must exist**: both mechanisms in the release build, each calling `run_backup`.
- **Allowed stubs**: none.
- **Unacceptable substitutes**: an always-on service for automatic backup; a first sync driven by the Flutter engine.

## Risks & Mitigation

**Risk 1: Long emulator runs are slow in CI**
- *Risk*: the 50,000-asset run takes hours.
- *Mitigation*: it runs in the kill-loop group (`X`) on demand and nightly, not on every pull request.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Play acceptance of the user-initiated job and the dataSync service is still the E-27/c02 spike; a refusal changes AC-1 | Owner / E-27 | open | Medium |
| PT-17-3's throughput targets are proposed by 17's test spec; AC-52 bounds only kills and memory | E-27 owner | accepted-risk | Low |
