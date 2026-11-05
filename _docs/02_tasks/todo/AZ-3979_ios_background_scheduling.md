# iOS triggers: app open, foreground observer, background tasks, cancel on expiry

**Task**: AZ-3979_ios_background_scheduling
**Name**: iOS background scheduling
**Description**: Every iOS trigger (app open, foreground observer, `BGProcessingTask`, `BGAppRefreshTask`) calls the shared `run_backup`, and a task expiry calls `cancel()` so the run stops cleanly and resumes from the checkpoint
**Complexity**: 3 points
**Dependencies**: AZ-3975_ios_uniffi_static_library, AZ-3976_ios_photokit_asset_source, AZ-3973_ios_ac78_sibling_spike, AZ-3990_run_backup_loop, AZ-3991_backup_core_surface
**Component**: 18 ios_app [v1.1] (Swift platform layer)
**Tracker**: AZ-3979
**Epic**: AZ-3641 (E-28)
**Ref**: E-28/c07

## Dependencies

### Document Dependencies
- `_docs/02_document/components/17_android_app/description.md` §2 (`run_backup`, `cancel`), §5 (one run at a time).
- `_docs/02_document/components/18_ios_app/description.md` §2 (Triggers row), §5.
- Criterion from E-28/c01 (AC-78 iOS sibling) for IT-18-9.

## Problem

iOS gives background work only when it chooses and only for a short window (about 30 s to minutes, RESTRICT-27). The Rust loop runs only while the process runs. If a window expires mid-block without `cancel()`, the run can be killed with an inconsistent checkpoint; if two triggers start loops at once, the same asset could be sent twice. The Swift layer must start every run through the one shared entry point and stop it on expiry, without promising any background timing.

## Outcome

- App open, the foreground observer, `BGProcessingTask` and `BGAppRefreshTask` each call `run_backup`; none implements upload.
- On a task expiry the handler calls `cancel()`; the checkpoint stays consistent, at most one block is lost, and the next run continues.
- A trigger that fires while a run is active gets `Skipped(AlreadyRunning)`, and overlapping runs give one server file per asset.
- Background arrival times are recorded for the report and never asserted.

## Scope

### Included
- Registration and scheduling of `BGProcessingTask` and `BGAppRefreshTask`, the app-open start and the foreground observer, each calling `run_backup` with its trigger and the PhotoKit `AssetSource`.
- The expiry handler calling `cancel()` and completing the task after the run returns.
- Recording each background run's arrival time and outcome for the test report.
- Tests IT-18-4 and IT-18-9.

### Excluded
- The run mutex, checkpointing and idempotent keys (E-27/c06) and `cancel()` itself (E-27/c07).
- The iOS 26.1 background upload extension and system-owned `URLSession` uploads (deferred, 18 §1).
- UI progress (E-28/c08).

## Acceptance Criteria

**AC-1: Every trigger uses the shared loop**
Given the app with both background task kinds registered
When a run is started by app open, by the foreground observer, and by each task kind through the debug trigger command
Then each run goes through `run_backup` with its trigger, and no other code path uploads

**AC-2: Expiry stops the run cleanly**
Given a background run uploading a large asset
When the task window expires mid-block
Then the expiry handler calls `cancel()`, the run returns, the checkpoint is consistent, and the next run continues with at most one block sent again

**AC-3: Background timing recorded, not asserted (RESTRICT-27)**
Given scheduled background tasks over a test session
When the runs arrive
Then each arrival time and outcome is written to the report, and no test fails on timing

**AC-4: One run at a time (IT-18-9)**
Given 100 new assets and a foreground run in progress
When a `BGProcessingTask` is triggered
Then the second `run_backup` returns `Skipped(AlreadyRunning)`, 100 server files exist after both finish, and each asset's `client_key` equals `derive_client_key(device_id, asset_id, file_hash)`

## Non-Functional Requirements

**Reliability**
- Every run is resumable from the checkpoint; a suspended or expired app loses at most one block (18 §5, AC-52).

**Performance**
- Background starts are best effort; the iOS memory budget of PT-18-1 applies to background windows.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Trigger-to-run mapping for the four triggers | Each calls `run_backup` with its trigger |
| AC-2 | Expiry handler | Calls `cancel()` once and completes the task after the run returns |

## Blackbox Tests

Appium XCUITest on DEV-IPHONE with a debug build for task triggering (harness E-28/c11a); triggers use the debug command, never wall-clock waiting; no device means `NOT-RUN(H-IOS)`.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Both task kinds registered | Four triggers | Each runs `run_backup` | IT-18-4, FT-P-31 |
| AC-2 | Large asset uploading | Expire window mid-block | `cancel()`; consistent checkpoint; <= 1 block re-sent | IT-18-4, NFT-RES-27 |
| AC-3 | Scheduled tasks | Arrival times | Recorded only | IT-18-4 |
| AC-4 | 100 new assets; foreground run | Trigger `BGProcessingTask` | `Skipped(AlreadyRunning)`; 100 files; derived keys | IT-18-9 |

## Constraints

- One upload implementation for every entry point (AC-52); the Swift layer schedules and cancels only.
- No claim that the backlog completes in the background (18 §5).

## Risks & Mitigation

**Risk 1: iOS kills the task before cancel takes effect**
- *Risk*: the process is suspended before the run returns.
- *Mitigation*: the checkpoint and the stored batch `commit_id` make the next run idempotent; AC-2 and AC-4 check counts, not timing.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| IT-18-9 has no AC id until E-28/c01 records the AC-78 iOS sibling; AC-4 is written against the shared mutex and key rules meanwhile | Owner via E-28/c01 | open | Low |
| 17 gives `RunContext{trigger, deadline, network_policy}`; the iOS trigger values and the deadline for a background window are not listed in 18 | E-27 / E-28 | open | Low |
