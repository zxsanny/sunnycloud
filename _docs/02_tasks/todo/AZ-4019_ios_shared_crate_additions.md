# Shared crate additions for iOS [v1.1]: `group_id`, `content_identifier`, `NeedsDownload`, checkpoint columns

**Task**: AZ-4019_ios_shared_crate_additions
**Name**: Shared crate iOS additions [v1.1]
**Description**: Extend the shared `mobile_backup` crate for iOS: `AssetSource` descriptor fields `group_id` and `content_identifier`, an `open_read` outcome `NeedsDownload` for iCloud-only assets that keeps them pending with a reason, and the matching checkpoint columns, without changing Android behaviour
**Complexity**: 3 points
**Dependencies**: AZ-3989_mobile_backup_checkpoint_db, AZ-3990_run_backup_loop, AZ-3991_backup_core_surface, AZ-3974_ios_live_photo_form_spike
**Component**: 17 android_app (crate `mobile_backup`, shared with 18) [v1.1]
**Tracker**: AZ-4019
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c06b

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/17_android_app/backup_core.md` — section "Planned 1.1.0 additions [v1.1]" (this task produces them and bumps the contract to 1.1.0).
- `components/18_ios_app/description.md` §2 (`group_id`, `NeedsDownload`), §4 (`content_identifier`, `group_id` columns).
- Decision of E-28/c02 (AZ-3974): stored form of edited renders and Live Photos, meaning and filling rule of both columns.

## Problem

iOS reuses the shared crate (ADR-013), but PhotoKit differs from MediaStore: an original and its edited render are separate resources linked by a `group_id`, assets have a PhotoKit identifier (`content_identifier`), and iCloud-optimised assets are not on the phone until downloaded (`NeedsDownload`). 18 §4 adds two checkpoint columns. The crate and its checkpoint belong to 17, so E-28 cannot add them (AZ-3974, AZ-3976 and AZ-3981 flag this). This row is split from rows c05 (checkpoint DB) and c06 (`AssetSource` and the loop); those rows keep their refs and stay v1.0-only, so Android v1.0 never waits for an iOS decision.

## Outcome

- `AssetDescriptor` gains optional `group_id` and `content_identifier`; Android passes none and behaves exactly as in v1.0.
- `open_read` may raise `NeedsDownload`: the asset stays pending with the reason `needs_download`, is not counted as skipped, is not retried again in the same run, and is tried on the next run.
- `status` reports pending assets waiting on `NeedsDownload` with that reason (field form recorded in `backup_core.md`); the sum rule still holds.
- A forward checkpoint migration adds `content_identifier` and `group_id` to `asset`; a v1.0 checkpoint migrates with both empty.
- Resources sharing a `group_id` are linked through the existing `linked_from` rule as decided by E-28/c02.
- `backup_core.md` moves to 1.1.0 with these rows; Kotlin and Swift bindings regenerate without manual edits.

## Scope

### Included
- Descriptor fields, the new host error, the pending reason in status, the migration, the group linking rule from E-28/c02.
- Binding regeneration and the drift checks of E-27/c18 and E-28/c03 updated.

### Excluded
- The PhotoKit source (E-28/c04), the iCloud download gating (E-28/c09), iOS reinstall relinking (E-28/c10).
- Any change to Android's MediaStore source (none is needed).

## Acceptance Criteria

**AC-1: Android unchanged [v1.1]**
Given the v1.0 Android test suites of E-27/c06 and E-27/c07 and descriptors without the new fields
When they run against the 1.1.0 crate
Then every result is the same as on 1.0.0

**AC-2: Fields stored [v1.1]**
Given descriptors with `group_id` and `content_identifier` from a fake iOS source
When a run lists and backs them up
Then both values are stored on each asset row and survive a restart

**AC-3: `NeedsDownload` keeps the asset pending [v1.1]**
Given 20 assets whose `open_read` raises `NeedsDownload` and 10 local assets
When a run completes
Then the 10 are uploaded, the 20 are pending with the reason `needs_download` and not skipped, no asset is retried twice in the run, `visible = uploaded + pending + skipped` holds, and the next run tries the 20 again

**AC-4: Checkpoint migration [v1.1]**
Given a checkpoint written by the 1.0.0 crate with 1,000 rows
When the 1.1.0 crate opens it
Then the migration adds both columns empty and every row keeps its state, hash, key and ids

**AC-5: Grouped resources linked [v1.1]**
Given an original and an edited render with the same `group_id`
When both are backed up
Then the server holds two entries linked by the rule of E-28/c02, not two unrelated files

## Non-Functional Requirements

**Compatibility**
- Contract `backup_core.md` 1.1.0 is a minor version: only optional additions.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | v1.0 unit suites on the 1.1.0 crate | Same results |
| AC-2 | Store and reload of the new fields | Values kept |
| AC-3 | `NeedsDownload` with a fake source | Pending with reason; sum holds; retried next run |
| AC-4 | Migration of a 1.0.0 checkpoint | Columns added; rows intact |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-5 | Native test binary, real server, grouped resources | Backup | Two linked entries | IT-18-10 (edited leg) |

## Constraints

- Nothing of this task is in v1.0 builds' behaviour: with no iOS host, every addition is inert (ADR-016).

## Risks & Mitigation

**Risk 1: E-28/c02 changes the linking rule**
- *Risk*: the decision needs more than `linked_from`.
- *Mitigation*: AC-5 follows the recorded decision; a larger change is a new E-27 row.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The meaning and filling rule of `group_id` and `content_identifier` wait for E-28/c02 (AZ-3974) | E-28 owner | open | Medium |
| The status field form for the pending reason is not stated in 18; this task records it in `backup_core.md` | E-27 / E-28 owners | open | Low |

## Contract

This task produces/implements the 1.1.0 additions of the contract at `_docs/02_document/contracts/17_android_app/backup_core.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
