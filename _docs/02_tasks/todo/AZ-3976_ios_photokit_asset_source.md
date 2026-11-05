# PhotoKit AssetSource: change tokens, edited renders, access levels, open_read

**Task**: AZ-3976_ios_photokit_asset_source
**Name**: PhotoKit AssetSource
**Description**: The Swift implementation of the shared `AssetSource` callback over PhotoKit: opaque change tokens, original and edited renders linked by `group_id`, the visible-id list for limited access, access levels and reading exported resources
**Complexity**: 5 points
**Dependencies**: AZ-3975_ios_uniffi_static_library, AZ-3974_ios_live_photo_form_spike, AZ-3990_run_backup_loop, AZ-4019_ios_shared_crate_additions
**Component**: 18 ios_app [v1.1] (Swift platform layer)
**Tracker**: AZ-3976
**Epic**: AZ-3641 (E-28)
**Ref**: E-28/c04

## Dependencies

### Document Dependencies
- `_docs/02_document/components/17_android_app/description.md` §2 — `AssetSource` callback (`assets_after`, `current_token`, `list_visible_ids`, `open_read`, `access_level`) of the shared crate.
- `_docs/02_document/components/18_ios_app/description.md` §2 — iOS differences.
- Decision of E-28/c02 (stored form and `group_id`, `content_identifier`).

## Problem

The shared backup loop (`run_backup`, E-27/c06) asks an `AssetSource` which assets are new, which are visible, and for their bytes. On iOS these answers come from PhotoKit, which has its own change history (`PHPersistentChangeToken`), separate resources for an original and its edited render, a limited-library mode where only picked photos are visible, and exported resources instead of file descriptors. If the Swift side gets any of this wrong, photos are missed, uploaded twice, or picked photos never back up (AC-53).

## Outcome

- The shared loop gets new assets after a stored change token, with the token kept opaque to Rust.
- An edited photo reaches the server as two linked entries, not a duplicate (AC-46 iOS).
- Under limited access only visible photos are listed, and photos picked later are found by the visible-id diff and uploaded on the next run (AC-53).
- Access level is read again at the start of every run; Denied pauses the run.

## Scope

### Included
- `assets_after` and `current_token` over PhotoKit with an opaque `ChangeToken(bytes)` wrapping `PHPersistentChangeToken`; Rust only stores and returns it.
- Original and edited renders reported as separate resources linked by a `group_id` (form per E-28/c02).
- `list_visible_ids` returning every asset the app can see now, so the shared loop can diff it against the `asset` table when the access level or the visible count changed.
- `access_level` with `Full`, `Limited` and `Denied`, read again at the start of every run.
- `open_read` returning an exported resource for upload; an iCloud-only asset raises `NeedsDownload` (the opt-in gating is E-28/c09).
- The IT-18-2 test (AssetSource side) and the edited-pair leg of IT-18-10.

### Excluded
- The backup loop, diff logic and checkpoint writes (E-27/c06), and the checkpoint columns themselves (E-27 change, see Flagged concerns).
- The coverage banner text and the picker button in the UI (E-28/c08).
- iCloud download and its Wi-Fi and charging gating (E-28/c09); triggers (E-28/c07).
- Live Photo pairing beyond the linked-resource upload (deferred; E-28/c02).

## Acceptance Criteria

**AC-1: New assets after a change token**
Given a stored change token and 5 photos added after it
When the shared loop asks for assets after that token
Then exactly those 5 are returned with a new opaque token, and asking again with the new token returns none

**AC-2: Edited photo is two linked resources**
Given an edited photo (original plus render)
When the assets are listed and backed up
Then two resources are reported with the same `group_id`, and the server holds two linked entries, not two unrelated files (IT-18-10 edited leg)

**AC-3: Limited access lists visible photos only (AC-53)**
Given "Selected Photos" authorization with 20 of 300 assets visible
When the visible ids are listed
Then exactly the 20 visible ids are returned and the access level is `Limited`

**AC-4: Picked photos back up on the next run (AC-53)**
Given limited access with 20 photos backed up
When the user picks 10 more and the next run starts
Then the next run's visible-id diff queues the 10 picks and uploads them, and the server holds 30 files

**AC-5: Access level read every run**
Given a run in progress under `Full`
When the user switches to `Denied` and the next run starts
Then the next run reads `Denied` at its start and ends as `Paused(PermissionDenied)`; switching back to `Full` lets the following run continue

**AC-6: Reading an exported resource**
Given a locally stored photo and an iCloud-only photo
When the shared loop opens each for reading
Then the local photo returns readable bytes equal in length to the resource, and the iCloud-only photo raises `NeedsDownload` without starting a download

## Non-Functional Requirements

**Performance**
- `open_read` streams the resource; no whole-library or whole-file buffering on the Swift side (AC-52 memory bound, PT-18-1).

**Reliability**
- PhotoKit permission changes can happen at any time; they take effect at the next run start (18 §7).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Change token round trip through the opaque wrapper | Same token bytes back; Rust never parses them |
| AC-5 | Mapping of PhotoKit authorization states to `Full`, `Limited`, `Denied` | One level per state |

## Blackbox Tests

Appium XCUITest on DEV-IPHONE (H-IOS) or the simulator against the real server stack (harness E-28/c11a); no device means `NOT-RUN(H-IOS)`.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Token stored, 5 new photos | Assets after token | 5 returned; repeat returns 0 | |
| AC-2 | Edited photo | Listing and backup | Same `group_id`; two linked entries | IT-18-10 (edited leg) |
| AC-3 | Selected Photos, 20 of 300 | Visible ids | 20 ids; `Limited` | IT-18-2, FT-P-35 |
| AC-4 | Pick 10 more | Next run | 10 uploaded; 30 server files | IT-18-2, FT-P-35 |
| AC-5 | Switch to Denied, then Full | Next runs | `Paused(PermissionDenied)`, then continues | IT-18-2 |
| AC-6 | Local and iCloud-only photo | `open_read` | Bytes; `NeedsDownload` | IT-18-8 |

## Constraints

- No upload or protocol logic on the Swift side; Swift only answers the `AssetSource` callback (18 §1).
- Same server layout and collision suffix as 17 (`Camera Uploads/<device label>/`, suffix from the asset id).

## Risks & Mitigation

**Risk 1: Limited access hides older picks from the change token**
- *Risk*: a token cannot reveal photos that become visible later.
- *Mitigation*: AC-4 checks the visible-id diff path, which the shared loop runs when the level or count changes.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| 17's `AssetSource` descriptor has no `group_id` or `content_identifier`, and `open_read` raises only `AssetGone` (no `NeedsDownload`); the shared trait and checkpoint need an E-27 change that no E-27 task lists | E-27 owner | open | Medium |
| Live Photo leg of IT-18-10 stays BLOCKED until E-28/c02 decides the stored form | E-28/c02 | open | Low |
