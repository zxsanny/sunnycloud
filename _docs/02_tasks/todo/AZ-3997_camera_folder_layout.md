# Camera folder names: asset-id collision suffix and `ParentGone` recovery

**Task**: AZ-3997_camera_folder_layout
**Name**: Camera folder names and recovery
**Description**: Name every upload in `Camera Uploads/<device label>/` so two assets with the same display name never collide (`<stem> (<asset_id>)<ext>`), and recreate the camera folder and retry once when the server reports it missing or deleted
**Complexity**: 2 points
**Dependencies**: AZ-3990_run_backup_loop, AZ-3991_backup_core_surface, AZ-3960_sync_tree_operations, AZ-3767_sc_names
**Component**: 17 android_app (crate `mobile_backup`)
**Tracker**: AZ-3997
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c13

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `create_folder`, `list_children`, `ParentNotFound`, `ParentGone` (E-23/c08).
- `_docs/02_document/contracts/shared_names/` — display-name rules (E-05/c01).
- `_docs/02_document/contracts/17_android_app/backup_core.md` — wire table row `ParentNotFound`/`ParentGone` (this task contributes it).

## Problem

Phones produce many files with the same name (`IMG_0001.jpg` in DCIM and in Pictures, RAW and JPEG pairs). The server's conflict-copy naming is only a last resort (canonical-names Step 4 default; 17 §4): the app must choose collision-free names itself from the asset id. If the user deletes the camera folder on the web, the next upload gets `ParentNotFound` or `ParentGone`; the app must recreate the folder and retry once instead of failing every asset.

## Outcome

- `upload_name` = the MediaStore display name; when another asset in the checkpoint already uses that name in the folder, `<stem> (<asset_id>)<ext>`.
- 0 server-named conflict copies in normal operation.
- On `ParentNotFound` or `ParentGone`: the camera folder is recreated, its node id replaced in `cursor`, and the asset retried once.

## Scope

### Included
- Name choice and its storage on the asset row; name rules through helper 06 (E-05/c01).
- Folder recovery and the single retry.
- Test IT-17-11 (Rust tier against the real server; the reinstall leg is E-27/c14).

### Excluded
- Folder creation on the first run (E-27/c06).
- Names after a reinstall from the rebuilt listing (E-27/c14).

## Acceptance Criteria

**AC-1: Collision suffix (IT-17-11)**
Given two assets both named `IMG_0001.jpg` (asset ids 101 and 202) and a third named `IMG_0002.jpg`
When they are backed up
Then the first processed asset keeps `IMG_0001.jpg`, the other becomes `IMG_0001 (202).jpg` or `IMG_0001 (101).jpg` by processing order, `IMG_0002.jpg` keeps its name, and the server holds 0 server-named conflict copies

**AC-2: Folder recreated once (IT-17-11)**
Given a backed-up camera folder that is deleted on the server before the next run
When the next upload gets `ParentGone` (or `ParentNotFound`)
Then the folder is recreated, its new node id is stored, the asset is retried once and succeeds, and the remaining assets go into the new folder

**AC-3: No endless retry**
Given a server that answers `ParentGone` again right after the folder was recreated
When the retry fails
Then the asset is not retried a second time in this run, and the failure appears in `last_error` with a correlation id

## Non-Functional Requirements

**Reliability**
- Names are chosen once and stored, so a retried upload keeps its name.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Name choice with duplicates in the checkpoint | Suffix from the asset id; stored |
| AC-3 | Recovery with a fake `SyncApi` failing twice | One retry only |

## Blackbox Tests

Native test binary of `mobile_backup` with sync-core against the real server container; the emulator run is in AZ-3673.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Two `IMG_0001.jpg`, one `IMG_0002.jpg` | Backup | Suffix applied; 0 conflict copies | IT-17-11, AC-46 |
| AC-2 | Camera folder deleted | Next run | Folder recreated; retried once | IT-17-11 |

## Constraints

- The server's conflict-copy name stays a last resort; the app never renames on the server after commit.

## Risks & Mitigation

**Risk 1: Suffix collides with a user file**
- *Risk*: the user created a file named like a suffixed name.
- *Mitigation*: the server's conflict-copy rule applies as the last resort.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| An edit uploads as a new file (AC-46) with the same display name and the same asset id as its original, so the suffix rule yields the original's name again; the name of an edited file is not stated | E-27 owner | open | Medium |
