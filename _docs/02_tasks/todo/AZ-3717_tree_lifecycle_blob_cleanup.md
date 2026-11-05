# Tree account lifecycle hook and blob owner cleanup

**Task**: AZ-3717_tree_lifecycle_blob_cleanup
**Name**: Tree lifecycle hook and blob cleanup
**Description**: Implement `AccountLifecycleHook` for the tree (root node at creation, tree rows at deletion) and `BlobOwnerCleanup::remove_owner_rows` for the GC sweeper
**Complexity**: 3 points
**Dependencies**: AZ-3706_tree_skeleton, AZ-3708_tree_folder_mutations, AZ-3709_tree_publish_core, AZ-3768_contracts_part1, AZ-3770_contracts_part3, AZ-3729_blob_registry_gc_queue, AZ-3730_gc_sweeper
**Component**: 02_tree
**Tracker**: AZ-3717
**Epic**: AZ-3640 (E-13)
**Ref**: E-13/c13

## Problem

A new account needs exactly one root node. A deleted account must lose all its tree rows within 15 minutes (AC-27) without one long transaction, and every blob must be queued for removal in the same transaction as the row that referenced it, so no file is forgotten (AC-30). When the GC sweeper removes a blob, the revision row pointing at it must go too, whatever its state, or the `RESTRICT` key would block collection of deleted files.

## Outcome

- `initialize(tx, account)` inserts the account root.
- `erase_batch(account, limit)` deletes revision rows and queues each blob `AccountDeleted`, `Due::Now` in the same short transaction, then nodes leaf first and the root last, and reports an honest `remaining`.
- `remove_owner_rows(tx, blob_id)` deletes the revision that points at the blob, current or not; a repeat is a no-op.

## Scope

### Included
- `AccountLifecycleHook::initialize` and `erase_batch` without the account advisory lock.
- `remaining` = nodes + revisions + the account's blobs still queued (`GcQueue::pending_for_account`).
- `BlobOwnerCleanup::remove_owner_rows`, relying on `ON DELETE SET NULL` for `nodes.current_revision_id`.

### Excluded
- Driving the hooks (07's `DeletionPurger`, E-17); late registration of the lists (E-22/c03); the sweeper itself (E-10/c10).

## Acceptance Criteria

**AC-1: Root at account creation**
Given a new account
When `initialize` runs in the account-creation transaction
Then exactly one root node exists for it, and a second root insert for that account is refused

**AC-2: Rows and GC rows move together (AC-27, AC-30)**
Given an account with 1,000 nodes and 600 revisions in state `deleting`
When `erase_batch(limit = 50)` runs in a loop
Then after every batch no revision row has been removed without its blob's `blob_gc` row (`AccountDeleted`, `Due::Now`) in the same transaction, no surviving revision's blob is queued by the erase, and no advisory lock was taken

**AC-3: Honest remaining and restart**
Given the AC-2 loop, killed once between batches and restarted, with the GC sweeper running later
When `remaining` is read after each batch
Then it never increases, the root is deleted last, the restarted run finishes, and `remaining` reaches 0 only after the sweeper has removed all the account's files

**AC-4: Owner cleanup removes any revision state**
Given a current revision, a replaced revision and the last revision of a tombstoned file
When `remove_owner_rows` is called for each blob, and again for one of them
Then each revision row is gone, the tombstoned file's `current_revision_id` is cleared by the key, `RESTRICT` never blocks, and the repeat changes nothing

## Non-Functional Requirements

**Reliability**
- Short transactions only; a batch is bounded by `limit`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | `remaining` arithmetic | Sum of the three counts |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | New account | `initialize` (IT-02-23) | One root | FT-P-19 |
| AC-2 | 1,000 nodes, 600 revisions | Erase loop (IT-02-23) | Rows and gc rows in one tx; no lock | NFT-RES-30, NFT-SEC-27 |
| AC-3 | Kill between batches; sweeper later | Remaining per batch (IT-02-23) | Non-increasing; root last; 0 after sweep | AC-27 |
| AC-4 | Three revision states | `remove_owner_rows` x4 (IT-02-23) | Rows gone; pointer cleared; repeat no-op | AC-30 |

## Constraints

- Hook and cleanup traits come from `contracts` (E-00/c03, E-00/c05); this task implements them as the shared lifecycle contract defines.
- Real E-10 `GcQueue` and `GcSweeper` on one PostgreSQL and a temp blob directory. The cross-component deletion run is pipeline AZ-3665.

## Risks & Mitigation

**Risk 1: Deadlock with a cascade batch**
- *Risk*: erase and cascade lock the same nodes.
- *Mitigation*: both are short batches; the database resolves and the batch retries once.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `data_model.md` §12.2 says the tree's erase batches still stamp the feed and adjust quota; 02 §2 `erase_batch` states neither | E-13 owner | open | Low |
