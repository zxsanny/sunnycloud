# `run_backup` loop: change token, visible-id diff, hashing ahead, run mutex, idempotent uploads

**Task**: AZ-3990_run_backup_loop
**Name**: run_backup loop
**Description**: The one Rust backup loop for Android and iOS: one run per process, new assets from the change token, a visible-id diff for limited-access picks, hashes saved before upload, uploads through sync-core with stable per-asset keys, and an asset marked uploaded only after the commit acknowledgement
**Complexity**: 5 points
**Dependencies**: AZ-3989_mobile_backup_checkpoint_db, AZ-3959_http_sync_client_connect, AZ-3960_sync_tree_operations, AZ-3961_sync_hash_file, AZ-3962_large_upload_executor, AZ-3963_small_upload_batch, AZ-3766_sc_ids
**Component**: 17 android_app (crate `mobile_backup`)
**Tracker**: AZ-3990
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c06

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `hash_file`, `upload`, `stage_small`, `batch_commit`, `create_folder` (E-23/c07, c08, c09a, c09b, c10).
- `_docs/02_document/contracts/13_sync_core/host_traits.md` — `FileSource`, `ProgressSink`, `CheckpointStore` (E-23/c14).
- `_docs/02_document/contracts/shared_ids_hashing/` — `derive_client_key` (E-04/c01).
- `_docs/02_document/contracts/17_android_app/backup_core.md` — `run_backup`, `RunContext`, `BackupOutcome`, `AssetSource` (this task contributes them).

## Problem

Every Android trigger (MediaStore change, 15-minute catch-up, app open, first sync) must run the same upload code (AC-52), and the foreground app and the background worker must never create two server files for one photo (AC-78, interaction risk R11). The loop must find new photos by an opaque change token, but a token cannot reveal older photos that become visible later under limited access (AC-53). Hashes must be saved so 10 forced kills re-hash at most 1% of 50,000 assets, and an asset must count as uploaded only after the server acknowledged its commit (AC-52).

## Outcome

- `run_backup(RunContext, AssetSource)` with the in-process run mutex (AC-78 layer 2) and idempotent uploads (AC-78 layer 3).
- New assets found by `assets_after` from the stored change token; a visible-id diff when the access level or visible count changed since the last run, and on every catch-up run.
- Hashes computed `android.hash_ahead` assets ahead and stored before each upload starts.
- Uploads through sync-core: files of at most 2 MiB through the batch path with a stored random `commit_id`, larger files through the block path with a resume row; every commit carries `client_meta` with device and asset id.
- An asset becomes `uploaded` (with node and revision ids) only after the commit acknowledgement; `NothingToSend` ends it at 0 bytes.

## Scope

### Included
- The `AssetSource` callback trait and `AssetDescriptor`, `ChangeToken` (opaque bytes) as in `backup_core.md`.
- The run mutex: one run per process; a concurrent call returns `Skipped(AlreadyRunning)` with no `SyncApi` call; released when the run ends or the process dies (no lease).
- Change-token cursor per volume in `cursor`; new rows stored before the cursor moves, so a kill after listing loses no asset.
- Access level and visible count re-read at the start of every run; visible-id diff as above; unknown ids queued as `new`.
- Hash ahead (`android.hash_ahead`, 8) through sync-core `hash_file` over a `FileSource` built on `open_read`; the hash stored on the row (`hashing` then hashed) before upload.
- `client_key = derive_client_key(device_id, asset_id, file_hash)`, stored on the row and reused on every retry.
- Routing by size (staged path at most 2 MiB, block path above), batches of at most `batch_commit_max` entries with the record saved before the first staging (through `CheckpointStore`).
- Target folder `Camera Uploads/<device label>/`: created through sync-core if missing, its node id kept in `cursor`, upload name = display name (the collision suffix and folder recovery are E-27/c13).
- An asset whose change token moved and whose hash differs is uploaded again with the latest hash (the link to the original is E-27/c14).
- `RunContext.deadline` reached or `cancel()` flag set → `Partial{remaining}`.
- Memory bound: block buffers only (at most 4 x 2 MiB), no whole-file read.

### Excluded
- Wire-error mapping to pauses and skips, `status`, `set_server`, `cancel` and the UniFFI surface (E-27/c07).
- Collision suffix and `ParentGone` recovery (E-27/c13); `linked_from` and reinstall rebuild (E-27/c14).
- The Kotlin `AssetSource` and entry points (E-27/c09, c10); emulator runs (E-27/c10, c11, pipeline AZ-3673).
- The [v1.1] iOS additions to `AssetSource` and the checkpoint (E-27/c06b).

## Acceptance Criteria

**AC-1: One run at a time (IT-17-1)**
Given a 100-asset fake `AssetSource` and a fake `SyncApi`
When two `run_backup` calls start within 1 ms in one process, a third starts after the first ended, and a fourth starts after a simulated process restart during a run
Then one of the first two returns `Completed` or `Partial` and the other `Skipped(AlreadyRunning)` having made 0 `SyncApi` calls, the third runs, and the fourth runs without waiting for any lease

**AC-78: Idempotent uploads when the mutex is bypassed (component part of interaction risk R11)**
Given two separate processes, each with its own checkpoint database, running `mobile_backup` over the same 100-asset fixture against the real server, and a batch variant with 300 assets of 1 KiB to 1 MiB
When both processes back up at the same time
Then the server holds exactly 100 (and 300) files with 0 duplicates and 0 conflict copies, both processes derive the same `client_key` per asset, the slower one gets the same session or `NothingToSend`, and every batch `commit_id` is a random UUIDv7 stored before its first staging and never equal to an asset's `client_key` (IT-17-3)

**AC-2: New assets from the change token**
Given a stored change token and 250 new assets behind it
When a run lists them in pages
Then 250 `new` rows exist before the stored token advances to `current_token()`, and the token bytes stored equal the bytes the source returned

**AC-3: Visible-id diff (AC-53)**
Given limited access with 20 visible assets, all uploaded, and then 10 older assets picked whose change tokens are below the cursor
When the next run starts (visible count changed) and, separately, a catch-up run starts with no change
Then the first run queues exactly the 10 older assets as `new`, the catch-up run performs the diff and queues nothing, and a non-catch-up run with unchanged level and count performs no diff

**AC-4: Hash saved before upload**
Given 50 assets and `hash_ahead` 8
When the run proceeds and is stopped after 20 uploads, then restarted
Then at no time are more than 8 assets hashed beyond the one being uploaded, every hashed asset's content hash was stored before its upload began, and the restarted run hashes none of the assets already hashed

**AC-5: Uploaded only after the acknowledgement**
Given a fake `SyncApi` that acknowledges a commit and a checkpoint write that fails right after it (the process is then restarted)
When the next run starts
Then the asset is uploaded again with the same `client_key`, the fake answers `NothingToSend`, the row becomes `uploaded` with 0 bytes sent, and no second file exists

**AC-6: Edits and pending rows (IT-17-9, Rust tier)**
Given an asset edited twice within one change-token tick and a source that hides pending and trashed rows until they are final
When the run processes them
Then the edited asset yields one upload of its latest hash, pending and trashed rows are uploaded only after they become final, and the change token round-trips as opaque bytes

**AC-7: Routing and client metadata**
Given assets of 0 bytes, 2 MiB and 2 MiB + 1, and 120 small assets with `batch_commit_max` 50
When they are uploaded
Then the first two take the staged path, the 2 MiB + 1 asset takes the block path with a `session` row, the small assets go in batches of at most 50, and every commit carries `client_meta` with the device and the asset id

**AC-8: Target folder**
Given a signed-in account with no `Camera Uploads/<device label>/` folder
When the first run starts and later runs follow
Then the folder is created once through sync-core, its node id is stored in `cursor`, and later runs reuse it without another create

## Non-Functional Requirements

**Performance**
- Memory at most 150 MB during uploads (AC-52; measured on the emulator by E-27/c11 PT-17-2).

**Reliability**
- Every checkpoint change is one transaction; a kill at any point leaves a state the next run continues from.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Two concurrent calls, a later call, a call after restart (fake source and fake `SyncApi`) | One runs, one `Skipped(AlreadyRunning)` with 0 calls; later runs proceed |
| AC-2 | Paged listing then token advance | Rows stored before the token moves; bytes equal |
| AC-3 | Diff on count change and on catch-up; no diff otherwise | 10 queued; 0 queued; no diff |
| AC-4 | Hash-ahead window and restart | At most 8 ahead; nothing re-hashed |
| AC-5 | Ack then failed checkpoint write, restart | Same key; `NothingToSend`; uploaded at 0 bytes |
| AC-6 | Double edit in one tick; pending and trashed rows | One upload of the latest hash; held rows later |
| AC-7 | Size routing and batch size | Staged, block and batch paths as stated; `client_meta` present |
| AC-8 | First and later runs | One create; node id reused |

## Blackbox Tests

Native test binaries of `mobile_backup` with sync-core against the real server container (real PostgreSQL), as IT-17-3 allows; emulator legs are in E-27/c10, E-27/c11 and AZ-3673.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-78 | Two processes, own checkpoints, 100 assets; 300-asset batch variant | Concurrent backup | 100 and 300 server files; same keys; stored `commit_id` | IT-17-3, FT-P-76 |
| AC-5 | One process killed after the commit acknowledgement | Restart and rerun | One server file; 0 bytes on rerun | AC-52 |

## Constraints

- One upload implementation: Kotlin and Dart never call upload routes (AC-52, checked by E-27/c11 IT-17-16 and AZ-3673 AC-6).
- Upload protocol, retries, refresh and batching rules belong to sync-core; this task only drives them.
- Camera backup never targets a vault folder.

## Runtime Completeness

- **Capability**: the shared mobile uploader run (`run_backup`) on sync-core's `HttpSyncClient`.
- **Production code that must exist**: the loop above calling the real sync-core operations and the SQLite checkpoint.
- **Allowed stubs**: fake `AssetSource` and fake `SyncApi` in unit tests only.
- **Unacceptable substitutes**: any upload code in Kotlin or Dart, a second uploader, a test-only loop, a fixture-backed `AssetSource` in a release build.

## Risks & Mitigation

**Risk 1: Worker and UI in two processes**
- *Risk*: the in-process mutex does not serialize runs across processes.
- *Mitigation*: AC-78 proves the idempotent layer alone gives one file per asset; E-27/c01 records the process model.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| What happens to rows of assets that leave the visible set (deleted on the phone, or un-picked under limited access) and how they count in `visible = uploaded + pending + skipped` is not stated | E-27 owner | open | Medium |
| Where `device_id` (input of `derive_client_key`) comes from on Android is only implied (`cursor` holds it; 17 §4) | E-27 owner | open | Low |
