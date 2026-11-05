# Reinstall rebuild and edited-photo links

**Task**: AZ-3998_reinstall_rebuild
**Name**: Reinstall rebuild and linked edits
**Description**: After a reinstall, rebuild the checkpoint from the server listing of this device's camera folder (`client_meta`), fall back to hashing plus batched `lookup_revisions`, so already backed-up photos send 0 media bytes, and upload an edited photo as a new file linked to its original
**Complexity**: 5 points
**Dependencies**: AZ-3990_run_backup_loop, AZ-3992_android_auth_token_vault, AZ-3993_kotlin_asset_source, AZ-3994_kotlin_entry_points, AZ-3997_camera_folder_layout, AZ-4003_android_emulator_harness, AZ-3960_sync_tree_operations, AZ-3716_tree_revision_lookups
**Component**: 17 android_app (crate `mobile_backup`)
**Tracker**: AZ-3998
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c14

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `list_children`, `lookup_revisions` (1,000 hashes per call) (E-23/c08).
- `_docs/02_document/contracts/02_tree/` — `lookup_revisions` and the `client_device` listing filter (E-13/c12).
- `_docs/02_document/contracts/17_android_app/backup_core.md` — `rebuild_checkpoint`, `RebuildReport` (this task contributes them).
- Record of E-27/c03 (asset-id stability on edits).

## Problem

A reinstalled app has an empty checkpoint. AC-46 requires 0 bytes of media uploaded for 5,000 already backed-up assets, and an edited photo to be linked to its original (same MediaStore asset id with a new content hash), not duplicated. The server keeps per-revision `client_meta` (device, asset id, link to original), so the checkpoint can be rebuilt from the listing; when that is not usable, the app hashes local assets and asks the server with batched lookups. There is no commit-by-hash: content stored by another device is not looked for (canonical-names Step 4 default).

## Outcome

- `rebuild_checkpoint(listing page callback) -> RebuildReport` from the listing of `Camera Uploads/<device label>/` with `client_meta`.
- Fallback: hash local assets and call `lookup_revisions` with up to 1,000 hashes per request; a hash found in this device's folder links the asset with 0 bytes uploaded.
- An asset with a known asset id and a new hash uploads as a new file whose `client_meta` links to the original (`linked_from`); a new asset id is a new unlinked file.
- `RebuildReport` fields recorded in `backup_core.md`.

## Scope

### Included
- Rebuild on the first run after sign-in when the checkpoint is empty and the device's folder exists.
- The listing path, the fallback path and the choice between them.
- `linked_from` on the asset row and in `client_meta` for edits (also in normal runs).
- Names from the rebuilt listing so two assets keep their earlier names (IT-17-11 reinstall leg).
- Test IT-17-12 and the reinstall leg of IT-17-11.

### Excluded
- Cross-device dedupe (out of scope); the server lookup itself (E-13/c12).
- iOS reinstall (E-28/c10, through this logic).

## Acceptance Criteria

**AC-1: Reinstall sends 0 media bytes (IT-17-12, AC-46)**
Given DS-21 with 5,000 backed-up assets
When the app is uninstalled, reinstalled and signed in again
Then `rebuild_checkpoint` returns a `RebuildReport`, every asset is linked to its server revision, and the proxy log shows 0 media bytes uploaded for the 5,000

**AC-2: Fallback with batched lookups (IT-17-12)**
Given the same 5,000 assets and `client_meta` not usable for the rebuild
When the rebuild runs
Then local assets are hashed and exactly 5 lookup requests of at most 1,000 hashes are sent, and 0 media bytes are uploaded

**AC-3: Edited photo linked (IT-17-12, AC-46)**
Given 100 assets edited with the same asset id and a new hash
When they are backed up
Then each uploads as a new file whose `client_meta` links to the original revision, and an asset with a new asset id uploads as an unlinked file

**AC-4: No cross-device lookup**
Given the same photo also uploaded by a second device of the account
When this device rebuilds and backs up
Then this device's copy is uploaded as its own file (two blobs, per the accepted deferral) and the second device's copy is not matched

**AC-5: Names kept after reinstall (IT-17-11 reinstall leg)**
Given two assets earlier stored as `IMG_0001.jpg` and `IMG_0001 (202).jpg`
When the app is reinstalled and rebuilt
Then both rows keep their earlier names and nothing is uploaded

## Non-Functional Requirements

**Performance**
- Rebuild of 5,000 assets through the listing within the 30-minute IT-17-12 budget (NFT-PERF-22).

**Reliability**
- A kill during the rebuild leaves a state the next run continues from (NFT-RES-28).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Rebuild from a scripted listing with `client_meta` | Rows linked to revisions |
| AC-2 | Fallback with 5,000 hashes | 5 batches of at most 1,000 |
| AC-3 | Known asset id with new hash; new asset id | Linked new file; unlinked file |

## Blackbox Tests

Appium and `adb` on the emulator against the real server with the proxy byte log (harness E-27/c19).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 5,000 backed-up assets | Uninstall, reinstall, sign in | 0 media bytes | IT-17-12, FT-P-32, NFT-PERF-22 |
| AC-2 | `client_meta` unusable | Rebuild | 5 lookups; 0 media bytes | IT-17-12 |
| AC-3 | 100 edited assets | Backup | Linked new files | IT-17-12, AC-46 |
| AC-4 | Same photo on two devices | Backup | Two blobs; no match | IT-17-12 |
| AC-5 | Suffixed names on the server | Reinstall | Names kept; 0 uploads | IT-17-11 |

## Constraints

- No commit-by-hash and no cross-device lookup (canonical-names Step 4).

## Risks & Mitigation

**Risk 1: An OEM changes asset ids on edit**
- *Risk*: edits become unlinked new files on that device.
- *Mitigation*: reported by E-27/c03, not worked around.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| When the listing path counts as "not usable" (so the fallback runs) is not stated | E-27 owner | open | Low |
| `RebuildReport` fields are not stated; this task records them in `backup_core.md` | E-27 owner | open | Low |
| The name of an edited file that shares display name and asset id with its original is open (see E-27/c13) | E-27 owner | open | Medium |
