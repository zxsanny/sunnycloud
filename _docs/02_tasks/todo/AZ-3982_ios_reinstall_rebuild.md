# Reinstall rebuild verification on iOS

**Task**: AZ-3982_ios_reinstall_rebuild
**Name**: iOS reinstall rebuild
**Description**: After deleting and reinstalling the iOS app, the shared checkpoint rebuild over the camera folder listing (with hash plus batched `lookup_revisions` as fallback) recognises every backed-up asset, so no media byte is uploaded again
**Complexity**: 2 points
**Dependencies**: AZ-3976_ios_photokit_asset_source, AZ-3978_ios_auth_session_provider_rule, AZ-3998_reinstall_rebuild, AZ-4019_ios_shared_crate_additions
**Component**: 18 ios_app [v1.1]
**Tracker**: AZ-3982
**Epic**: AZ-3641 (E-28)
**Ref**: E-28/c10

## Dependencies

### Document Dependencies
- `_docs/02_document/components/17_android_app/description.md` §4 (Reinstall rule, server layout, collision suffix).
- `_docs/02_document/components/18_ios_app/description.md` §4.

## Problem

A reinstall wipes the app's checkpoint. Without a rebuild, the next run would upload the whole library again. The shared crate rebuilds the checkpoint from the listing of the device's camera folder (per-revision `client_meta`), falling back to hashing local assets and batched `lookup_revisions` (1,000 hashes per request). On iOS the asset identifiers, names and the linked edited renders come from PhotoKit, so the rebuild must be verified with the iOS `AssetSource`.

## Outcome

- After a reinstall of a device with 2,000 backed-up assets, 0 media bytes are uploaded.
- The camera folder listing is used first; hashing plus batched lookups is the fallback.
- The asset-id collision suffix for equal display names is kept, so no second file appears for an existing one.

## Scope

### Included
- Wiring the iOS sign-in result and PhotoKit `AssetSource` into the shared rebuild on the first run after reinstall.
- Verification of the rebuild on iOS with the proxy log (test IT-18-6).

### Excluded
- The rebuild logic itself (E-27/c14) and the server lookup (E-13/c12).
- Live Photo relinking (deferred to the E-28/c02 decision).

## Acceptance Criteria

**AC-1: 0 media bytes after reinstall (AC-46 iOS)**
Given 2,000 assets already backed up from this iPhone
When the app is deleted, reinstalled, signed in and a run completes
Then the proxy log shows 0 media bytes uploaded and the server still holds 2,000 files with no duplicates

**AC-2: Listing first, lookups as fallback**
Given the reinstalled app
When the checkpoint is rebuilt
Then the listing of `Camera Uploads/<device label>/` is read first, and only assets it cannot match are hashed and looked up with `lookup_revisions` in requests of at most 1,000 hashes

**AC-3: Collision suffix kept**
Given two backed-up assets with the same display name (one stored with the asset-id suffix)
When the checkpoint is rebuilt and a run completes
Then both map to their existing server files and no new file is created

## Non-Functional Requirements

**Performance**
- Reinstall timing is recorded only; no time bound is stated for iOS (performance G2-G13 list reinstall timing as owner parameters).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None beyond E-27/c14's shared tests (not repeated, 18 test tiers) | — |

## Blackbox Tests

Appium XCUITest on DEV-IPHONE against the real server stack, proxy log (harness E-28/c11a); no device means `NOT-RUN(H-IOS)`.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 2,000 backed-up assets | Delete, reinstall, sign in, run | 0 media bytes; 2,000 files | IT-18-6, NFT-RES-28 |
| AC-2 | Reinstalled app | Rebuild request log | Listing first; lookups of <= 1,000 | IT-18-6 |
| AC-3 | Equal display names | Rebuild and run | Existing files matched; 0 new | IT-18-6 |

## Constraints

- No cross-device dedupe: only this device's camera folder is matched (`canonical-names.md`).

## Risks & Mitigation

**Risk 1: PhotoKit identifiers change after reinstall**
- *Risk*: the local asset ids no longer equal the ids in `client_meta`.
- *Mitigation*: the hash plus `lookup_revisions` fallback still matches by content; AC-1 counts bytes, not id matches.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Whether PhotoKit local identifiers stay stable across a reinstall is not stated; the iOS counterpart of E-27's asset-id stability spike does not exist | E-28 / owner | open | Low |
| Edited renders and Live Photos relink according to E-28/c02, not yet decided | E-28/c02 | open | Low |
