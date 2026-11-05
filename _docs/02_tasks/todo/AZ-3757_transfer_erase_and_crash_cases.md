# Transfer erase hook and the crash, stamp-order and moved-parent cases

**Task**: AZ-3757_transfer_erase_and_crash_cases
**Name**: Transfer erase hook and interaction cases
**Description**: Implement the transfer `AccountLifecycleHook` (sessions, staged rows, receipts and their blobs) and run the component cases for a crash between blob write and commit, effects after the stamp, and a commit after the parent moved
**Complexity**: 3 points
**Dependencies**: AZ-3736_transfer_resume_abort, AZ-3737_transfer_stage_small, AZ-3738_transfer_commit_upload, AZ-3754_transfer_commit_batch, AZ-3756_transfer_janitors_reservations, AZ-3708_tree_folder_mutations, AZ-3713_tree_move_vs_edit, AZ-3768_contracts_part1, AZ-3729_blob_registry_gc_queue
**Component**: 03_transfer
**Tracker**: AZ-3757
**Epic**: AZ-3643 (E-14)
**Ref**: E-14/c12

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/03_transfer/commit_service.md` — effect behaviour under test.
- `_docs/02_document/contracts/02_tree/tree_commit.md` — stamp-last rule.

## Problem

Account deletion must remove upload sessions, staged rows and receipts in short transactions and queue each blob in the same transaction (AC-27, AC-30). Three interaction risks have their single-boundary part here: R1 (a crash between blob write and commit must leave no revision pointing at a missing blob, AC-68), AC-69 (03's effects must add nothing after 02's stamp) and AC-70 (a commit names the node by id; `parent_id` is only a hint). The pipelines AZ-3655, AZ-3658 and AZ-3659 check the cross-component outcomes.

## Outcome

- `erase_batch(account, limit)` deletes rows with `RETURNING blob_id` and queues each blob `AccountDeleted`, `Due::Now` in the same transaction, evicts progress entries, and reports `remaining`.
- The crash, statement-order and moved-parent cases pass against real 02 and 04.

## Scope

### Included
- `AccountLifecycleHook::initialize` (no-op) and `erase_batch` without the account advisory lock; reservations vanish with the rows (07a erases its row; no `release_reservation`); `remaining` = rows left in `upload_sessions` and `commit_receipts`.
- IT-03-15, IT-03-17 (cases a restart-commit and b staged), IT-03-18, IT-03-19.

### Excluded
- The reaper's TTL release after a crash (E-14/c11); the real-guard refusals of put, stage and commit (E-14/c15); orphan sweeping (E-10/c11).

## Acceptance Criteria

AC-1 and AC-2 are local to this task. AC-68, AC-69 and AC-70 keep their numbers from `acceptance_criteria.md`.

**AC-1: Erase in short batches (AC-27, AC-30)**
Given an account in `deleting` with 30 open sessions, 200 staged rows, 5,000 receipts and progress entries
When `erase_batch(limit = 100)` runs in a loop
Then each batch deletes rows and queues their blobs with `AccountDeleted`, `Due::Now` in the same transaction (no deleted row without its `blob_gc` row), takes no advisory lock, calls no `release_reservation`, `remaining` reaches 0, and the progress entries are evicted

**AC-2: Starts refused during deletion**
Given the account of AC-1
When a session start is attempted
Then it returns `AccountNotActive`

**AC-68: Crash between blob write and commit (R1, component part)**
Given (a) a large upload with all blocks acked and the service dropped before `commit_upload`, and (b) a staged upload whose decorator fails after `write_complete` and before the row transaction
When (a) restarts and commits, and (b) is checked
Then (a) shows 0 revisions and 0 nodes before the commit, `get_upload` lists all blocks or the missing ones, and the commit succeeds; (b) leaves 0 rows, 0 reservation and 1 orphan file for 04's sweep; and every revision's blob file exists and decrypts

**AC-69: Effects add nothing after the stamp**
Given the statement log and reader/writer decorators
When a single commit and a batch commit run
Then the `on_begin` lookup, the `on_entry` session lock, blob flip and row delete, and the `on_batch` receipt insert all precede the stamp, and 0 `BlobReader`/`BlobWriter` calls happen between BEGIN and COMMIT

**AC-70: Commit after the parent moved**
Given an upload for existing file f (base r1) whose folder F is moved to G between `start_upload` and `commit_upload`, with `parent_id = F` in the request, and separately a new-file commit whose given parent was tombstoned
When each commit runs
Then the new revision lands on f (same node id) under G with 0 conflict copies, and the new-file commit returns `ParentGone`

## Non-Functional Requirements

**Reliability**
- Short transactions only; the erase never holds a lock across batches.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | `remaining` arithmetic | Sessions plus receipts left |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 30 sessions, 200 staged, 5,000 receipts | Erase loop (IT-03-15) | Rows and gc rows together; remaining 0 | FT-P-19, NFT-RES-30 |
| AC-2 | Account `deleting` | Start (IT-03-15) | `AccountNotActive` | NFT-SEC-27 |
| AC-68 | Drop before commit; failure after staged write | Restart-commit; check (IT-03-17) | No dangling revision; orphan left for 04 | NFT-RES-34 |
| AC-69 | Statement log, decorators | Single and batch commit (IT-03-19) | All effect statements before stamp; no I/O in tx | NFT-RES-35 |
| AC-70 | Folder moved mid-upload; tombstoned parent | Commit (IT-03-18) | Same id under G; `ParentGone` | FT-N-42 |

## Constraints

- Real 02, 04 and 07a on one PostgreSQL; the service drop is a new service instance over the same database and blob root.
- Before E-15 lands the account state check uses a port-level test implementation (E-14/c15 repeats AC-2 with the real guard).

## Risks & Mitigation

**Risk 1: Orphan files counted as leaks**
- *Risk*: case (b) leaves a file on disk.
- *Mitigation*: expected by design; 04's orphan sweep (IT-04-14) removes it after 48 h.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `ParentGone` (03) and `ParentDeleted` (02, `parent_deleted` 410) name one condition; the commit contract maps one to the other | E-14 owner | accepted-risk | Low |
