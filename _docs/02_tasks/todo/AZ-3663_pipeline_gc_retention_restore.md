# Blob GC, retention and operator restore pipeline (F12)

**Task**: AZ-3663_pipeline_gc_retention_restore
**Name**: GC, retention and restore pipeline
**Description**: The scheduler's GC, purge and sweep cycles remove deleted content after retention, and `admin restore` and `admin gc-run` act on the real tree and blobstore, end to end against the real components
**Complexity**: 3 points
**Dependencies**: AZ-3742_test_harness_skeleton, AZ-3765_sc_clock, AZ-3831_internal_router_test_clock, AZ-3789_composition_root, AZ-3791_serve_start_stop, AZ-3792_janitor_scheduler, AZ-3796_epoch_restore_gc_commands, AZ-3729_blob_registry_gc_queue, AZ-3730_gc_sweeper, AZ-3743_orphan_sweeper, AZ-3714_tree_cascade_purge_janitors, AZ-3715_tree_restore, AZ-3717_tree_lifecycle_blob_cleanup, AZ-3845_media_janitors_erase_hook, AZ-3778_feed_horizon_lifecycle, AZ-3756_transfer_janitors_reservations, AZ-3659_pipeline_tree_conflicts, AZ-3652_initial_structure
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3663
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/p11

## Problem

Retention spans the delete transaction (02 queues `blob_gc`), 04's `GcSweeper` calling the `BlobOwnerCleanup` list (02, 08) that 12 registers late, 02's tombstone purge raising 05's horizon, and the operator's `admin restore` and `admin gc-run`. Each janitor is unit-tested with doubles; the late list and the scheduler exist only in 12. No planned task proves that deleted content really disappears after retention and can be restored before it.

## Outcome

- Deleted and replaced originals stay restorable for the retention period and are removed by the scheduler afterwards; derived blobs go at once when their content is gone.
- `admin restore` brings a file back as an ordinary change, and `admin gc-run` runs the same cycle on demand.
- Every component operation of flow F12 has a production caller; at least one integration test runs F12 through the real binaries with the test clock.

## Scope

### Included
- Flow order and seams (`system-flows.md` F12 steps 1-5):
  1. Delete or overwrite (E-22/p07, p03) → `blob_gc` row due at now plus retention.
  2. Scheduler (12) → `GcSweeper::run_cycle` → unlink → every `BlobOwnerCleanup::remove_owner_rows` in the list registered by 12 (02 revision rows, 08 derived rows) → blob and gc rows removed. `admin gc-run` calls the same cycle.
  3. Scheduler → 02 purge janitor (tombstones older than 90 days) → `FeedHorizon::raise_purge_horizon`; `SessionReaper` (03) and `OrphanSweeper` (04) on their intervals; 08 `DerivedJanitor` queues derived blobs `Due::Now`.
  4. `admin restore --account --path --at` → `TreeRestore::restore_revision` → revision re-pointed (placement `Original` or `Restored`), `BlobRegistry` reinstates the blob and clears the gc row in the same transaction → feed stamp.
- The end-to-end integration test of the steps above with the test-clock offset.

### Excluded
- Account erase (E-22/p12), database restore and epoch (E-22/p13), user-facing trash (none in v1.0).

## Acceptance Criteria

AC-1..AC-6 are local to this task. No interaction risk of `interaction-risks.md` is owned here (R6 restore vs retention is AC-73 in E-22/p13).

### Wiring and flow criteria

**AC-1: Retention then removal**
Given retention of 7 days and a deleted file
When the test clock is moved to day 6, then past day 7, with the scheduler running
Then on day 6 the blob file and its rows still exist, and after day 7 the GC cycle has unlinked the file and removed the revision, blob and gc rows; `admin gc-run` reports the same counts when run instead of waiting

**AC-2: Derived blobs follow their content**
Given a photo with thumbnails whose last revision is collected
When the derived janitor and GC cycles run
Then its derived rows and blobs are removed without waiting for retention

**AC-3: Restore by path and time**
Given a file deleted at T and a file whose folder was also deleted
When `admin restore` runs for each at a time before T within retention, and for a third file after its blob was collected
Then the first returns to its original folder (placement `Original`), the second lands in `Restored` at the account root, both reach devices as ordinary feed changes, and the third reports "beyond retention" and changes nothing

**AC-4: Tombstone purge raises the horizon**
Given a tombstone older than 90 days (test clock)
When the purge cycle runs and a device polls with a cursor below the new horizon
Then the tombstone is gone and the poll gets 410 `resync_required` with `below_purge_horizon`

**AC-5: Crash between unlink and row removal**
Given a due blob
When the server is killed after the unlink and before the row removal, and restarted
Then the next cycle removes the rows without error

**AC-6: Every flow operation has a production caller**
Given the source tree
When the wiring check searches non-test code
Then it finds scheduler registrations of `GcSweeper`, the 02 purge janitor, `OrphanSweeper` and `DerivedJanitor`, the `BlobOwnerCleanup` list (02 and 08) passed to 04 by 12, a production caller of `FeedHorizon::raise_purge_horizon`, and `admin restore` and `admin gc-run` calling `TreeRestore::restore_revision` and `GcSweeper::run_cycle`

## Non-Functional Requirements

**Performance**
- A janitor cycle with nothing due runs 3 queries or fewer and finishes within 20 ms at p95 (PT-12-2).

**Reliability**
- Retention outside 0-90 days refuses start (config validation).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-6 | Static caller check over non-test sources | Each named registration and call has a production caller |

## Blackbox Tests

Integration through the real binaries; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Retention 7 d, deleted file, test clock | Day 6, day 7+, `gc-run` | Kept, then removed; same counts from `gc-run` | AC-30 |
| AC-2 | Photo with thumbnails, last revision collected | Derived janitor, GC | Derived removed at once | |
| AC-3 | Deleted files, one past retention | `admin restore` x3 | Original, Restored, beyond retention | AC-30, AC-41 |
| AC-4 | Tombstone older than 90 d | Purge, poll below horizon | 410 `below_purge_horizon` | |
| AC-5 | Due blob | Kill after unlink, restart | Rows removed, no error | |

## Constraints

- Owner: component 12 (`src/server/`): scheduler and late-list registrations for this flow, the two commands' wiring, and the flow's end-to-end tests among 12's integration tests. No SQL, blob I/O or business rule in 12.
- Tests run the real `sunnycloud` binary against PostgreSQL 18 and a real blob volume; time moves only through the test-clock offset.
- Downstream: E-31/c25 (FT-P-16..18), E-31/c32 (FT-N-35, FT-N-40) and E-31/c39 should depend on this task.

## Risks & Mitigation

**Risk 1: Cleanup list missing an owner**
- *Risk*: 08's cleanup is not in the list, so derived rows outlive their blob files.
- *Mitigation*: AC-2 checks derived rows after GC.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| GC interval and trigger (G-21) is a decompose-time gap (E-31/c03) | E-31 / owner | open | Low |
