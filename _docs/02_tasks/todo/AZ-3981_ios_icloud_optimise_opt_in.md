# iCloud "Optimise Storage" opt-in: NeedsDownload, Wi-Fi and charging gating

**Task**: AZ-3981_ios_icloud_optimise_opt_in
**Name**: iCloud Optimise Storage opt-in
**Description**: Photos stored only in iCloud are listed as pending with a reason and downloaded for backup only when the user has opted in and the phone is on Wi-Fi and charging
**Complexity**: 3 points
**Dependencies**: AZ-3976_ios_photokit_asset_source, AZ-3980_ios_flutter_ui, AZ-3990_run_backup_loop, AZ-4019_ios_shared_crate_additions
**Component**: 18 ios_app [v1.1] (Swift platform layer)
**Tracker**: AZ-3981
**Epic**: AZ-3641 (E-28)
**Ref**: E-28/c09

## Dependencies

### Document Dependencies
- `_docs/02_document/components/18_ios_app/description.md` §1 (opt-in default off, Wi-Fi and charging), §2 (`open_read` raises `NeedsDownload`), §7.
- `_docs/02_document/components/17_android_app/description.md` §2 (`BackupStatus` counts: visible, uploaded, pending, skipped).

## Problem

With iCloud "Optimise Storage" on, many originals are not on the phone; reading them means a large download from iCloud first. Doing that silently would use mobile data and battery. 18 makes it an opt-in, off by default, and limits it to Wi-Fi and charging. Assets that cannot be read now must still be visible in the status as pending with a reason, so the user's counts add up.

## Outcome

- With the opt-in off, iCloud-only assets are listed as pending with a reason and nothing is downloaded from iCloud.
- With the opt-in on, Wi-Fi and charging, those assets are downloaded and uploaded.
- With the opt-in on but without charging (or without Wi-Fi), they wait.
- The visible count always equals uploaded + pending + skipped.

## Scope

### Included
- Handling of `NeedsDownload` from `open_read` (raised by E-28/c04): pending state with its reason.
- The gating rule: download only when opt-in on and Wi-Fi and charging; re-checked when a run starts.
- The iCloud download of the original when the rule allows it, then the normal upload through the shared loop.
- Test IT-18-8.

### Excluded
- The setting switch in the UI (E-28/c08) and the `AssetSource` itself (E-28/c04).
- Any upload logic (shared loop, E-27/c06).

## Acceptance Criteria

**AC-1: Opt-in off downloads nothing**
Given 20 iCloud-only assets and the opt-in off (default)
When a backup run completes
Then the 20 are listed as pending with a reason and 0 downloads from iCloud took place

**AC-2: Opt-in on with Wi-Fi and charging**
Given the same 20 assets, the opt-in on, Wi-Fi and the phone charging
When a backup run completes
Then the 20 are downloaded from iCloud and uploaded, and the server holds them

**AC-3: Opt-in on without charging waits**
Given the opt-in on and Wi-Fi but the phone not charging
When a backup run starts
Then the iCloud-only assets stay pending (waiting) and nothing is downloaded

**AC-4: Counts add up**
Given any of the states above
When the status is read
Then the visible count equals uploaded + pending + skipped

## Non-Functional Requirements

**Performance**
- The download of one original does not buffer the whole file in memory (PT-18-1 budget).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Gating rule with opt-in off and every network and charging state | No download |
| AC-2 | Gating rule with opt-in on, Wi-Fi, charging | Download allowed |
| AC-3 | Gating rule with opt-in on and one of Wi-Fi or charging missing | No download |

## Blackbox Tests

Appium XCUITest on DEV-IPHONE with an iCloud account (owner) against the real server stack (harness E-28/c11a); no device or account means `NOT-RUN(H-IOS)`.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 20 iCloud-only, opt-in off | Backup run | 20 pending with reason; 0 iCloud downloads | IT-18-8 |
| AC-2 | Opt-in on, Wi-Fi, charging | Backup run | 20 downloaded and uploaded | IT-18-8 |
| AC-3 | Opt-in on, not charging | Backup run | Waiting; 0 downloads | IT-18-8 |
| AC-4 | Each state | Status counts | visible = uploaded + pending + skipped | IT-18-8 |

## Constraints

- The opt-in is off by default (18 §1); no download on cellular.

## Risks & Mitigation

**Risk 1: Charging or network changes mid-download**
- *Risk*: the phone is unplugged during a long iCloud download.
- *Mitigation*: the rule is re-checked at run start; what happens to a download already in progress is a flagged concern.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `NeedsDownload` is not in the shared `AssetSource` error set (17 lists only `AssetGone`); it needs the same E-27 change as E-28/c04 | E-27 owner | open | Medium |
| The pending-reason value for iCloud-only assets and whether an in-progress iCloud download stops when charging or Wi-Fi drops are not stated in 18 | E-28 / owner | open | Low |
