# Kotlin `AssetSource` over MediaStore with original access and access levels

**Task**: AZ-3993_kotlin_asset_source
**Name**: Kotlin AssetSource
**Description**: The Kotlin implementation of the shared `AssetSource` callback over MediaStore: opaque change tokens from the MediaStore generation, visible assets only, the visible-id list for limited-access picks, access levels, and originals opened with `setRequireOriginal` when `ACCESS_MEDIA_LOCATION` is granted
**Complexity**: 5 points
**Dependencies**: AZ-3991_backup_core_surface, AZ-3992_android_auth_token_vault, AZ-4000_flutter_channel_bridge, AZ-4002_native_library_ci, AZ-4003_android_emulator_harness
**Component**: 17 android_app (Kotlin platform layer)
**Tracker**: AZ-3993
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c09

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/17_android_app/backup_core.md` — `AssetSource`, `AssetDescriptor`, `ChangeToken`, `AccessLevel`, `AssetGone` (E-27/c06, c07).
- Records of E-27/c02 (permission declarations) and E-27/c03 (asset-id stability).

## Problem

The Rust loop never touches Android APIs; it asks Kotlin which assets are new, which are visible and for their bytes (ADR-013). Android 14's "selected photos" access hides the library total and makes older picked photos visible without moving the change token (AC-53). Pending and trashed MediaStore rows must not be uploaded until they are final. Without `setRequireOriginal` and `ACCESS_MEDIA_LOCATION`, Android hands out a copy with the Exif location removed, so the server would not hold the original (AC-46 test IT-17-10).

## Outcome

- `assets_after`, `current_token`, `list_visible_ids`, `open_read`, `access_level` over MediaStore images and videos, with pending and trashed rows filtered and only the user's selected buckets returned.
- Access levels `Full`, `Limited` (selected photos), `Denied` read live at each call.
- `open_read` opens the original with `setRequireOriginal` when `ACCESS_MEDIA_LOCATION` is granted; otherwise the redacted copy is uploaded and the core reports `location_redacted`; a vanished asset raises `AssetGone`.
- The photo permission and `ACCESS_MEDIA_LOCATION` requested together; "selected photos" grants that expired lead to a request for full access.

## Scope

### Included
- MediaStore queries for images and videos; the generation encoded into the opaque `ChangeToken`.
- Filters: `IS_PENDING = 1` and `IS_TRASHED = 1` excluded until final; buckets limited to the user's selection from settings.
- `list_visible_ids` paging over the same filters.
- Permission requests (photo and video read, selected photos on Android 14+, `ACCESS_MEDIA_LOCATION`) per the E-27/c02 record.
- Tests IT-17-8 and IT-17-10.

### Excluded
- The diff logic and checkpoint writes (E-27/c06); the coverage banner and its tap (E-27/c15).
- Entry points and WorkManager constraints (E-27/c10).

## Acceptance Criteria

**AC-1: Limited access and newly picked photos (IT-17-8, AC-53)**
Given an API 34+ emulator with 300 photos and "Select photos" granted for 20 of them
When a run completes, 10 more older photos are picked, and the next run completes
Then the first run uploads 20, `status` reports `visible = 20` and `access_level = Limited`, and after the pick the next run uploads the 10 older photos (30 server files); no library total is ever returned by the source

**AC-2: Denied and full access**
Given the same app
When access is revoked to Denied and a run starts, then full access is granted and a run starts
Then the first run returns `Paused(PermissionDenied)` with no upload, and the second uploads the remaining visible photos

**AC-3: Pending and trashed rows held back (IT-17-9 source part)**
Given five new photos, one with `IS_PENDING = 1` and one with `IS_TRASHED = 1`
When a run lists assets, and again after both rows become final
Then the first listing returns three, and the later listing returns the two others

**AC-4: Original with location (IT-17-10)**
Given a JPEG with Exif GPS in MediaStore
When it is backed up once with `ACCESS_MEDIA_LOCATION` granted and once (fresh install) with it denied
Then the first server file's BLAKE3 equals the device original and its downloaded bytes carry the GPS data, and the second upload still succeeds with the redacted copy and `status.location_redacted` is true

**AC-5: Vanished asset**
Given an asset listed by the source and deleted before `open_read`
When the core opens it
Then `open_read` raises `AssetGone` and the asset is counted as `skipped_gone`

**AC-6: Buckets from settings**
Given photos in the Camera and Screenshots buckets and a third bucket, with only Camera and Screenshots selected
When a run lists assets
Then no asset of the third bucket is returned

## Non-Functional Requirements

**Performance**
- Listing pages do not load whole files; `open_read` returns a file descriptor read block by block by the core.

**Compatibility**
- API 30, 33 and 34+ emulator images; the selected-photos path only on 34+.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Filter on pending and trashed rows (Robolectric or instrumented MediaStore fixture) | Held back until final |
| AC-6 | Bucket filter | Unselected bucket absent |

## Blackbox Tests

Instrumented runs on the emulator (harness E-27/c19) calling `run_backup` with the real MediaStore source against the real server.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 300 photos, 20 selected, then 10 more | Two runs | 20, then 30 server files; no total | IT-17-8, FT-P-35, AC-53 |
| AC-2 | Denied, then full | Two runs | Paused; then uploads | IT-17-8 |
| AC-3 | Pending and trashed rows | Listings | 3, then 2 more | IT-17-9 |
| AC-4 | JPEG with GPS | Granted and denied runs | Equal hash with GPS; redacted with flag | IT-17-10, AC-46 |
| AC-5 | Deleted before read | Run | `skipped_gone` | FT-N-50 |

## Constraints

- Kotlin is the only code that queries MediaStore; Rust sees opaque tokens and file descriptors only.
- `photo_manager` is not used for background work (17 §5).

## Runtime Completeness

- **Capability**: MediaStore listing and original-file access on the device.
- **Production code that must exist**: the MediaStore-backed `AssetSource` used by every entry point.
- **Allowed stubs**: none in instrumented tests; fake sources only in Rust unit tests of other tasks.
- **Unacceptable substitutes**: a fixture or file-system scan instead of MediaStore in a release build.

## Risks & Mitigation

**Risk 1: Selected-photos grants expire**
- *Risk*: the user's pick lapses and the app silently sees fewer photos.
- *Mitigation*: access level and count are re-read every run (E-27/c06); the app asks for full access again.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The default bucket selection is not stated (the mockup shows "Camera, Screenshots") | E-27 owner | open | Low |
| Whether assets in unselected buckets count in `visible` is not stated | E-27 owner | open | Low |
