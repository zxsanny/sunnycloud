# Android camera backup pipeline (F16)

**Task**: AZ-3673_pipeline_android_camera_backup
**Name**: Android camera backup pipeline
**Description**: Every Android trigger runs the one shared Rust backup loop, which reads MediaStore through Kotlin, keeps its checkpoint and uploads through sync-core to the real server without duplicates, end to end on the emulator
**Complexity**: 5 points
**Dependencies**: AZ-3987_mediastore_asset_id_spike, AZ-3988_oem_background_s9_spike, AZ-3990_run_backup_loop, AZ-3991_backup_core_surface, AZ-3993_kotlin_asset_source, AZ-3994_kotlin_entry_points, AZ-3995_first_sync_job, AZ-3997_camera_folder_layout, AZ-3998_reinstall_rebuild, AZ-4000_flutter_channel_bridge, AZ-4003_android_emulator_harness, AZ-3960_sync_tree_operations, AZ-3961_sync_hash_file, AZ-3962_large_upload_executor, AZ-3963_small_upload_batch, AZ-3655_pipeline_large_upload, AZ-3656_pipeline_small_upload_batch, AZ-3672_pipeline_android_sign_in, AZ-3652_initial_structure
**Component**: 17 android_app (Flutter + Kotlin app, `mobile_backup` crate)
**Tracker**: AZ-3673
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/p02

## Problem

Camera backup chains the Kotlin entry points (MediaStore trigger, periodic catch-up, app-open kick, first-sync job), the `BackupWorker`, `BackupCore::run_backup` in the shared Rust crate, the Kotlin `AssetSource` callback, the checkpoint database, sync-core's uploads and the server's commit idempotency. E-27/c06 builds the Rust loop and E-27/c10 the Kotlin entry points, each against fakes. No planned task proves on a device image that a new photo reaches the real server once, from any trigger, across kills and reinstalls.

## Outcome

- New photos and videos on the emulator reach the real server under `Camera Uploads/<device label>/` from every trigger, with no duplicate across kills, concurrent runs and reinstalls.
- Every component operation of F16 has a production caller, and no upload path exists outside `run_backup`.
- At least one emulator test runs F16 against the real server stack.

## Scope

### Included
- Flow order and seams (`system-flows.md` F16 steps 1-6):
  1. Triggers: `MediaTriggerWorker` (content-URI trigger), `CatchUpWorker` (periodic), `AppOpenKick`, `FirstSyncJob` (user-initiated data transfer) → `BackupWorker` (unique work) → `BackupCore::run_backup(RunContext, AssetSource)`.
  2. `run_backup` (in-process mutex) → `AssetSource::{current_token, assets_after, list_visible_ids, open_read, access_level}` (Kotlin, MediaStore) → checkpoint DB (generation, hashes, staged keys, saved `commit_id`).
  3. Per asset: BLAKE3 hash saved to the checkpoint → `SyncApi::upload` (F4) or `stage_small` plus `batch_commit` (F5) with a stable `client_key` and the saved commit id → server.
  4. Checkpoint updated after the commit acknowledgement; pauses on quota, network policy and permission; skipped assets recorded.
  5. Reinstall → `BackupCore::rebuild_checkpoint` from the listing and `SyncApi::lookup_revisions` (`client_meta`).
  6. `BackupStatus` → Kotlin → EventChannel → Flutter status screen.
- The emulator test of the steps above.

### Excluded
- Sign-in and refresh (E-27/p01), iOS (E-28/p01), file browsing on Android (out of v1.0), vault (camera backup never targets a vault).

## Acceptance Criteria

AC-1..AC-6 are local to this task. AC-78 keeps its number from `acceptance_criteria.md`.

### Wiring and flow criteria

**AC-1: New photos back up from the MediaStore trigger**
Given a signed-in emulator and the real server
When 20 photos and 2 videos are pushed into the camera folder
Then all 22 are on the server under `Camera Uploads/<device label>/` within 60 s, each revision carries its `client_meta` (device, asset id, hash), and the Flutter status shows 22 uploaded

**AC-2: Every trigger uses the same loop**
Given new assets
When backup is started by app open, by the periodic catch-up after a missed trigger, and by the first-sync job
Then each run goes through `run_backup`, and the catch-up picks up assets the missed trigger skipped

**AC-3: Kills do not duplicate**
Given a 500-asset first sync
When the process is killed three times during the run and restarted
Then the backup completes with exactly 500 server files, and each kill re-hashes at most 1% of the bytes

**AC-4: Pauses and skips**
Given an account near its quota, a metered network with Wi-Fi-only on, and an asset deleted between listing and upload
When backup runs
Then it pauses with the quota message, waits on the metered network, and records the deleted asset as skipped

**AC-5: Reinstall sends nothing twice**
Given a fully backed-up device
When the app is reinstalled and signed in again
Then the checkpoint is rebuilt from the server and 0 bytes are uploaded for already backed-up assets

**AC-6: Every flow operation has a production caller**
Given the app sources (Kotlin, Rust)
When the wiring check searches non-test code
Then every Kotlin entry point calls `BackupCore::run_backup` and no other upload call exists, `run_backup` calls the `AssetSource` methods and `SyncApi::{upload, stage_small, batch_commit, lookup_revisions}`, and status reaches Flutter through the EventChannel

### Interaction criteria (Step 1.8)

**AC-78: Foreground and background together create one server file** (R11)
Given one new photo
When the app-open kick and the background worker start a backup at the same moment (20 rounds with a fresh photo each)
Then each round leaves exactly one server file for that asset, the second run reports `Skipped(AlreadyRunning)` or commits idempotently with the same `client_key` and commit id, and the checkpoint records one upload

## Non-Functional Requirements

**Performance**
- New photo on the server within 60 s on a Pixel and within 5 minutes at p95 on Samsung and Xiaomi (AC-9); devices are lab runs in E-31/c45.

**Reliability**
- Memory at most 150 MB during uploads.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-6 | Caller check over Kotlin and Rust sources | Every entry point calls `run_backup`; no second upload path |

## Blackbox Tests

Appium and `adb` on the emulator against the real stack; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Signed-in emulator | Push 22 assets | On server within 60 s with `client_meta` | AC-9 |
| AC-2 | New assets | App open, catch-up, first-sync job | All via `run_backup`; missed assets caught up | AC-52 |
| AC-3 | 500 assets | Kill 3 times | 500 files, at most 1% re-hash per kill | AC-46 |
| AC-4 | Near quota, metered, deleted asset | Run | Pause, wait, skip recorded | |
| AC-5 | Backed-up device | Reinstall | 0 bytes re-sent | AC-46 |
| AC-78 | One new photo per round | App-open and worker at once, 20 rounds | One server file each round | FT-P-76 |

## Constraints

- Owner: component 17 (`src/mobile/` and `src/mobile_backup/`): trigger-to-loop composition and this flow's emulator tests.
- No fake of sync-core or the server; MediaStore is the emulator's real one.
- The in-process mutex itself stays in E-27/c06; AC-78 checks the outcome on the server.
- Downstream: E-31/c27 (FT-P-29, 30, 32, 34, 35, 76), E-31/c41 and E-31/c45 should depend on this task.

## Risks & Mitigation

**Risk 1: Two runs in two processes**
- *Risk*: the foreground app and WorkManager run in different processes, so the in-process mutex does not serialize them.
- *Mitigation*: AC-78 relies on the server-side idempotency as the second line; the spike E-27/c01 confirms the process model.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| OEM background behaviour and catch-up timing (gate S9) is still a spike (E-27/c04) | E-27 | open | Medium |
| MediaStore asset-id stability on in-place overwrite is still a spike (E-27/c03) | E-27 | open | Low |
