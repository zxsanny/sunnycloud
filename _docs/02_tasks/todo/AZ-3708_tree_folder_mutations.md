# TreeService: create folder, rename, move and delete

**Task**: AZ-3708_tree_folder_mutations
**Name**: Tree node mutations
**Description**: Implement `create_folder`, `update_node` (rename, move with cycle check, case-only rename) and `delete_node` with the name rules, the account guard and the stamp-last tail
**Complexity**: 5 points
**Dependencies**: AZ-3706_tree_skeleton, AZ-3765_sc_clock, AZ-3766_sc_ids, AZ-3767_sc_names, AZ-3775_feed_stamp, AZ-3777_feed_long_poll, AZ-3705_tree_spike_names_effect_tests
**Component**: 02_tree
**Tracker**: AZ-3708
**Epic**: AZ-3640 (E-13)
**Ref**: E-13/c04

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` — port signatures and DTOs produced by E-00/c04.
- `_docs/02_document/contracts/02_tree/tree_service.md` — produced by this task (E-13/c12 adds `resolve_path` and `find_revisions`).

## Problem

Folders, renames, moves and deletes come from the web, desktop and CLI through 11. They must keep names unique per folder case-insensitively but stored as typed (AC-42), reject the names AC-37 forbids, never create a cycle, refuse a delete based on a stale revision, and publish each change to the feed exactly once. Nothing may be added under a folder whose tombstone has already committed.

## Outcome

- Each mutation runs in its own short transaction: account guard first, writes, feed stamp last, commit, then the wake-up.
- A case-only rename keeps the node id; a move cannot create a cycle; a delete with a stale base is refused with the current revision.
- Folder delete writes one tombstone and `cascade_pending`, never one large transaction.

## Scope

### Included
- `create_folder(account, parent, name)`: name rules through helper 06, parent row `FOR SHARE`, errors `NameInvalid(reason)`, `NameConflict`, `ParentDeleted`, `NotFound`, `NotAFolder`.
- `update_node(account, id, name?, parent?)`: rename and move; a move takes the account advisory lock and runs the recursive cycle check (depth about 30); errors add `Cycle` and `IsRoot`; no `base_revision`.
- `delete_node(account, id, base_revision)`: `EditedSince{current_revision}` on a stale base, idempotent on a tombstoned node, `IsRoot`, `NotFound`; folder delete sets one tombstone plus `cascade_pending` and updates the parent row.
- `AccountGuard::ensure_active` as the first statement; `FeedStamp::stamp` last (none on failure); `FeedNotifier::notify` after COMMIT.
- Unique violations mapped to `NameConflict`; retry once on deadlock (E-13/c02).

### Excluded
- Usage delta and GC queueing on delete (E-13/c07); marking descendants (E-13/c10).
- `resolve_path`, `find_revisions` (E-13/c12); the move-versus-edit race suite (E-13/c09).

## Acceptance Criteria

**AC-1: Server name rules (AC-37)**
Given the root folder
When folders are created with `a/b`, `a\0b`, `.`, `..`, a control character, a bidi-control character, a 256-byte name and an empty name, and with `CON`, `a:b`, `..a`, `trailing.`, `.hidden`, an emoji name, a 255-byte name and `A.txt`
Then each of the first group returns `NameInvalid(reason)` with 0 rows written, and each of the second group is stored and listed with its exact spelling

**AC-2: Case-insensitive uniqueness (AC-42)**
Given folder `Docs` exists
When `docs` is created, and when `é` is created in NFD after `é` in NFC
Then both return `NameConflict` (the unique index decides; no server error), unless E-13/c01 records an owner decision that the NFC/NFD pair are two names

**AC-3: Parent errors on create**
Given a random id, a tombstoned folder, a file and another account's folder
When `create_folder` uses each as the parent
Then it returns `NotFound`, `ParentDeleted`, `NotAFolder` and `NotFound`, and writes 0 rows

**AC-4: Rename and move rules**
Given the tree `R/A/B/f.txt`
When `f.txt` is renamed to `F.TXT`, A is moved into B, a folder is moved into its own descendant 30 levels deep, the root is renamed, and nodes are moved into a tombstoned folder, onto a taken name and into a file
Then the case-only rename keeps the node id with the new spelling, both cycle moves return `Cycle` (the deep check finishes within 100 ms), and the rest return `IsRoot`, `ParentDeleted`, `NameConflict` and `NotAFolder`

**AC-5: Move waits for the account lock**
Given a second connection holding the account's advisory lock
When `update_node` moves a node
Then the move blocks until that lock is released and then succeeds

**AC-6: Delete rules (AC-8)**
Given file f with current revision r2 and a folder F with 100 files
When f is deleted with base r1, then with base r2 twice, and F is deleted
Then the stale base returns `EditedSince{current_revision = r2}`, the correct base succeeds, the repeat succeeds with no further change, and F gets one tombstone with `cascade_pending` in one transaction

**AC-7: One stamp per success, none per failure**
Given each successful and each failed call in AC-1 to AC-6
When the account feed counter is read before and after
Then every success advanced it by exactly one, every failure left it unchanged, and the wake-up ran only after COMMIT

**AC-8: Guard first**
Given the `AccountGuard` port reports the account not active
When any of the three mutations is called
Then it returns `AccountNotActive` with 0 rows written and no seq consumed

**AC-9: Nothing added under a committed tombstone**
Given folder F
When `create_folder(F, x)` and `delete_node(F)` run concurrently 200 times
Then no run ends with a live child under a tombstoned F: either the create committed first and the child is under the tombstone for the cascade, or the create returned `ParentDeleted`

## Non-Functional Requirements

**Performance**
- Cycle check on a 30-level chain under 100 ms; it runs before the stamp so the hot counter row is held briefly.

**Reliability**
- `create_folder` and `delete_node` do not take the account advisory lock.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | Cycle detection on a chain and a self-move | Cycle reported; no false positive on siblings |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Root folder | Reject and accept lists (IT-02-1) | `NameInvalid` x8, 0 rows; accepted stored verbatim | FT-N-41, NFT-SEC-22 |
| AC-2 | `Docs`; NFC `é` | `docs`; NFD `é` (IT-02-1) | `NameConflict` x2 | FT-P-15, FT-N-04 |
| AC-3 | Four bad parents | `create_folder` (IT-02-3) | `NotFound`, `ParentDeleted`, `NotAFolder`, `NotFound` | |
| AC-4 | `R/A/B/f.txt` | Case rename, cycles, root, bad targets (IT-02-3) | Same id; `Cycle` < 100 ms; typed errors | FT-P-15 |
| AC-5 | Lock held elsewhere | Move (IT-02-3) | Blocks, then succeeds | |
| AC-6 | f at r2; F with 100 files | Deletes (IT-02-4) | `EditedSince`; idempotent; one tombstone | FT-P-12 |
| AC-7 | All runs above | Counter delta (IT-02-3) | +1 per success; 0 per failure | AC-69 |
| AC-8 | Guard refuses | Each mutation | `AccountNotActive`, nothing written | IT-02-10 |
| AC-9 | Folder F | 200 concurrent create/delete pairs | Never a live child under a tombstone | |

## Constraints

- Before E-15 lands, the `AccountGuard` port is backed by a test implementation at the port boundary (epic testing strategy); the real guard runs in E-13/c16.
- Real 05 `FeedStamp`/`FeedNotifier` on the same PostgreSQL.

## Risks & Mitigation

**Risk 1: Deadlock between delete and a non-locking batch**
- *Risk*: delete updates the parent row while a cascade or erase batch holds node locks.
- *Mitigation*: the database resolves it; the operation retries once (E-13/c02).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-1 NFC/NFD may make the AC-2 pair two names (E-13/c01) | Owner | open | Low |
| IT-02-10 text still lists "create_folder/update_node/delete_node do not call the guard" as a gap; resolved at the Step 5 gap pass (02 risks (b)) | tests.md writers | resolved | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/02_tree/tree_service.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
