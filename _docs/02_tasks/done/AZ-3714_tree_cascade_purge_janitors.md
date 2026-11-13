# Cascade and tombstone purge janitors with the purge horizon

**Task**: AZ-3714_tree_cascade_purge_janitors
**Name**: Tree cascade and purge janitors
**Description**: Implement `TreeMaintenance::cascade_batch` and `purge_tombstones`, restart-safe by query alone, raise the feed's purge horizon in the purge transaction, and wrap both as janitors for the scheduler
**Complexity**: 5 points
**Dependencies**: AZ-3708_tree_folder_mutations, AZ-3709_tree_publish_core, AZ-3768_contracts_part1, AZ-3729_blob_registry_gc_queue, AZ-3776_change_feed_read, AZ-3778_feed_horizon_lifecycle
**Component**: 02_tree
**Tracker**: AZ-3714
**Epic**: AZ-3640 (E-13)
**Ref**: E-13/c10

## Problem

A folder delete writes one tombstone and `cascade_pending`, so something must later mark the descendants in bounded batches. Tombstones older than the 90-day horizon must be purged, and the feed must learn the highest purged seq in the same transaction, or a device with an old cursor would miss deletions instead of being told to resync (AC-41). Both loops run in the API process with no leader and must survive a kill at any point.

## Outcome

- `cascade_batch(now, limit)` marks descendants of folders with `cascade_pending` in batches of `cascade_batch_size`, without new seq values, and clears the flag when a subtree is done.
- `purge_tombstones(now, limit)` deletes tombstones older than `tombstone_horizon` leaf first in batches of `purge_batch_size`, queues the blobs of purged revisions, and calls `FeedHorizon::raise_purge_horizon` as the last statement of each transaction.
- Both are `Janitor`s that drain until nothing is due or the tick's time budget ends, using system scope.

## Scope

### Included
- Cascade marking per `data_model.md` §5.1: `deleted_at` set, no new seq, subfolders get `cascade_pending`.
- Tombstone purge with `FeedHorizon::raise_purge_horizon`; blob queueing for purged revisions.
- `Janitor` wrappers (`name()`, `run_cycle(now)`) with the `tree_maintenance_interval` time budget; rows claimed without a leader; deadlock with `publish` retried once.
- Logs: WARN when a cascade batch is slower than the interval; INFO batch sizes, no names or paths.

### Excluded
- Usage deltas and `deleted` GC rows of cascaded files (E-13/c07).
- Scheduler registration (E-22/c06).

## Acceptance Criteria

**AC-1: Cascade in bounded batches**
Given folder F with 100 files deleted, `cascade_batch_size = 20`
When `cascade_batch(now, 20)` runs repeatedly
Then it returns 20, 20, 20, 20, 20 and then 0, all 100 files are marked deleted without new seq values, and F's `cascade_pending` is cleared

**AC-2: Purge is restart-safe (AC-27)**
Given `tombstone_horizon = 2 s`, `purge_batch_size = 10` and 100 tombstoned nodes past the horizon
When the first run is cut by dropping its connection after 2 batches and a second run follows
Then the second run finishes the rest, 100 nodes are purged in total, none twice, and the blobs of purged revisions are queued

**AC-3: Purge horizon raised in the same transaction (AC-41)**
Given the purge of AC-2
When the change feed is read with a cursor below the new horizon and with one at or above it
Then `account_feeds.purge_horizon` equals the highest purged seq (its update is the last statement of each purge transaction), the low cursor gets `ResyncRequired{below_purge_horizon}`, and the other gets a normal page

**AC-4: Janitor cycle drains and idles**
Given due work in two accounts, and separately no due work
When `run_cycle(now)` runs
Then it processes both accounts in one cycle up to the time budget, and with nothing due it writes nothing and reports 0

**AC-5: Cascade batch time (PT-02-4)**
Given a folder with 5,000 descendants on C-REF
When one `cascade_batch(now, 5000)` runs
Then it finishes within 2 s (fail above 5 s)

## Non-Functional Requirements

**Performance**
- Cascade batch of 5,000 nodes at most 2 s (fail 5 s), PT-02-4 on C-REF; reported `NOT-RUN(HW-REF)` until REF-SRV exists.

**Reliability**
- No loop state beyond `cascade_pending` and the tombstone timestamps; a restart resumes by query.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | Time-budget loop with a manual clock | Stops at the budget; idle cycle reports 0 |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | F with 100 files deleted | Cascade loop (IT-02-4) | 20 x5 then 0; no new seq | FT-P-12 |
| AC-2 | 100 tombstones, ms horizon | Interrupted purge, rerun (IT-02-20) | 100 purged once; blobs queued | NFT-RES-07 |
| AC-3 | After purge | Feed read below and above horizon (IT-02-20) | `ResyncRequired`; normal page | FT-P-13, FT-P-14, NFT-RES-08 |
| AC-4 | Two accounts; nothing due | `run_cycle` | Both processed; idle writes nothing | ST-02-2 |
| AC-5 | 5,000 descendants, C-REF | One batch (PT-02-4) | <= 2 s | |

## Constraints

- Maintenance runs in system scope through helper 07 (ADR-014); RLS stays on for user-scoped calls.
- The `Janitor` trait comes from `contracts` (E-00/c03).

## Risks & Mitigation

**Risk 1: Deadlock with a concurrent publish (E-13 risk 4)**
- *Risk*: a cascade batch and a 500-entry publish lock overlapping nodes.
- *Mitigation*: the database resolves it; one retry; batch size bounds the hold.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-05-3: `raise_purge_horizon` is non-decreasing (GREATEST); a lower seq leaves it unchanged, so purge batches that commit out of seq order are safe for AC-3 (resolved 2026-11-07) | E-12 | resolved | Low |
