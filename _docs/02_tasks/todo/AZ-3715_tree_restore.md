# TreeRestore: operator restore of a revision

**Task**: AZ-3715_tree_restore
**Name**: Tree operator restore
**Description**: Implement `TreeRestore::restore_revision` for `admin restore`: pick the revision current at a given time, reinstate its blob, re-point the existing revision row into the original folder or `Restored`, and publish the change
**Complexity**: 5 points
**Dependencies**: AZ-3709_tree_publish_core, AZ-3710_tree_conflict_rules, AZ-3711_tree_quota_gc_deltas, AZ-3716_tree_revision_lookups, AZ-3729_blob_registry_gc_queue
**Component**: 02_tree
**Tracker**: AZ-3715
**Epic**: AZ-3640 (E-13)
**Ref**: E-13/c11

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/02_tree/tree_service.md` — `resolve_path`, used for the path form of the input.

## Problem

AC-30 promises that an operator can restore a file by path and time within the retention period. Users have no trash and cannot download old revisions, so this is the only way back. `revisions.blob_id` is unique, so a restore must re-point the existing revision row, never insert a second one, and it must take the blob back out of the GC queue in the same transaction; if the original folder is gone, the file must land somewhere predictable (`Restored`, decision 4).

## Outcome

- `restore_revision(account, node id or path, at)` returns `RestoreOutcome{restored_revision, placement: Original | Restored}` or `BeyondRetention`, `FileMissing`, `NotFound`.
- One transaction under the account advisory lock: reinstate, re-point, quota delta, stamp, COMMIT; any refusal rolls everything back.
- The restored file reaches devices as an ordinary feed change.

## Scope

### Included
- Pick the latest revision of the node with `created_at <= at`.
- `BlobRegistry::reinstate_in_tx` (clears `blob_gc`, `gc_pending` to `committed`); a refusal rolls back the whole restore.
- `Original`: the original folder is live; the original node is revived if tombstoned (conflict-copy name if its name is taken) and gets the restored revision as current; a previously current revision is queued `Replaced`.
- `Restored`: the original folder is not live; a file node is created in a root folder named `Restored` (created on first use) and the revision row is re-parented to it.
- Quota delta +restored size −replaced current size; stamp over the touched nodes; COMMIT; notify (reaches nobody from the CLI process; held polls see the change on 05's 10 s recheck).
- Operator only, system scope.

### Excluded
- The `admin restore` command and its argument parsing (E-22/c10); path parsing rules (E-13/c12).

## Acceptance Criteria

**AC-1: Restore into the original folder (AC-30)**
Given a file with r1 at t1 and r2 at t2, deleted at t3, inside a live folder
When `restore_revision(account, path, at = t1 + ε)` runs
Then the revision count is unchanged, r1 is the current revision of the revived original node, and the placement is `Original`

**AC-2: Restore when the folder is gone**
Given the same file whose original folder was also deleted
When it is restored, and then a second file from another deleted folder is restored
Then each lands as a file node under one root folder `Restored` (created once), with its revision row re-parented, and the placement is `Restored`

**AC-3: Name taken and GC rows**
Given the original name was taken by a new file after the delete
When the restore runs
Then the restored node gets a conflict-copy name, a previously current revision is queued `Replaced`, and the restored blob's `blob_gc` row is gone with its state `committed`

**AC-4: Typed refusals roll back everything**
Given an `at` older than the retention period, a revision whose blob file is absent, and a reinstate refusal (its `blobs` row removed)
When each restore runs
Then they return `BeyondRetention`, `FileMissing` and a refusal, and each leaves no stamp, no quota change and no row change

**AC-5: Usage and feed on success (AC-74)**
Given a successful restore that replaces a current revision
When usage and the feed are read
Then usage changed by +restored size −replaced size and the counter advanced by exactly one

**AC-6: Serialised with commits**
Given a second connection holding the account's advisory lock
When a restore starts
Then it waits until the lock is released

## Non-Functional Requirements

**Reliability**
- No second revision row for a blob, ever (`revisions.blob_id` UNIQUE).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Revision choice for an instant | Latest `created_at <= at` |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | r1, r2, deleted at t3 | Restore at t1+ε (IT-02-21) | Count unchanged; `Original` | FT-P-18 |
| AC-2 | Folder deleted | Two restores (IT-02-21) | One `Restored` folder; re-parented rows | FT-P-18 |
| AC-3 | Name taken | Restore (IT-02-21) | Conflict-copy name; `Replaced` queued; gc row cleared | NFT-RES-09 |
| AC-4 | Old `at`; file absent; blob row removed | Restore x3 (IT-02-21) | Typed refusals; full rollback | |
| AC-5 | Replacing restore | Usage and counter | Exact delta; +1 | |
| AC-6 | Lock held elsewhere | Restore | Waits | |

## Constraints

- Real E-10 `BlobRegistry` (reinstate) and real 05/07a on one PostgreSQL; the CLI-level flow is pipeline AZ-3663.

## Risks & Mitigation

**Risk 1: Restore racing the GC sweeper**
- *Risk*: the sweeper unlinks the file while the restore reinstates it.
- *Mitigation*: reinstate runs in the restore transaction and a refusal rolls back; the sweeper's claim is 04's (E-10/c10).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| A path can match several tombstoned nodes (deleted, recreated, deleted again); which node a path restore picks is not stated | E-13 owner / Owner | open | Medium |
| How `FileMissing` is detected (which port reports an absent blob file) is not stated in 02 | E-13 owner, E-10 | open | Low |
