# BlobRegistry, BlobCatalog and the GcQueue single writer

**Task**: AZ-3729_blob_registry_gc_queue
**Name**: Blob registry, catalog and GC queue
**Description**: Row writes for blobs inside the caller's transaction, the account-scoped `BlobRef` lookup, reinstatement for operator restore, and the one function that queues a blob for removal
**Complexity**: 3 points
**Dependencies**: AZ-3723_blobstore_skeleton, AZ-3724_blob_file_format, AZ-3765_sc_clock
**Component**: 04 blobstore
**Tracker**: AZ-3729
**Epic**: AZ-3627 (E-10)
**Ref**: E-10/c09

## Problem

03, 08 and 02 change blob rows inside their own transactions (session start, derived output, commit, delete, restore). A replaced or deleted blob must stay for the retention period (AC-30, RESTRICT-10), and the GC queue row and the `gc_pending` state must never disagree. Downloads need the key material of a blob without seeing another account's blobs (AC-59).

## Outcome

- `BlobRegistry`: `register_in_tx` (state `uploading` or `committed`), `mark_committed_in_tx`, `reinstate_in_tx` (`Reinstated`, `BeyondRetention`, `FileMissing`; the file is checked before the database statements), `remove_row_in_tx`.
- `BlobCatalog::get_ref(account, blob)` returns `BlobRef{blob_id, size, wrapped_key, master_key_id}` or `None` for a foreign or unknown id.
- `GcQueue::schedule_in_tx(tx, blob, reason, Due::AfterRetention | Due::Now)` writes `blob_gc` and sets `gc_pending` together, with `due_at = now + retention` or `now`; `pending_for_account` counts an account's queued, unremoved blobs.

## Scope

### Included
- The four registry methods, the catalog read, the queue writer and its count.

### Excluded
- The sweeper that acts on the queue (E-10/c10); deciding what to queue (02, 03, 08).

## Acceptance Criteria

**AC-1: Rows follow the caller's transaction**
Given a caller transaction
When `register_in_tx(uploading)` then `mark_committed_in_tx` run and the transaction commits, and in a second case the caller rolls back
Then the first row is `committed`, and the rolled-back case leaves no row

**AC-2: Catalog is account scoped**
Given blobs of accounts A and B
When A asks `get_ref(A, own blob)`, `get_ref(A, B's blob)` and `get_ref(A, unknown id)`
Then the first returns its `BlobRef` with size, wrapped key and master key id, and the other two return `None`

**AC-3: Reinstate outcomes**
Given a blob in `gc_pending` within retention, a blob whose rows were removed after retention, and a committed row whose file is absent
When `reinstate_in_tx` runs for each
Then the first returns `Reinstated` with its queue row removed and state `committed`, the second `BeyondRetention`, and the third `FileMissing`; `remove_row_in_tx` deletes both the `blobs` and the `blob_gc` row

**AC-4: Scheduling writes both rows or neither**
Given retention 7 days, then retention 0, a `Due::Now` request, a caller rollback and a second schedule of an already queued blob
When `schedule_in_tx` runs for each
Then `due_at` is now + 7 days, now and now, state is `gc_pending` with a queue row, the rollback leaves neither change, and the duplicate keeps one row with the earliest `due_at`

**AC-5: Pending count is per account**
Given queued blobs of accounts A and B, some already removed
When `pending_for_account(A)` runs
Then it returns exactly A's queued blobs that are not yet removed

## Non-Functional Requirements

**Performance**
- `get_ref` is one indexed read; it is the only read of `blobs` on the request path.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | `due_at` computation with the manual clock | now + retention, or now |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Caller transactions | IT-04-8 register/commit/rollback | `committed`; no row after rollback | |
| AC-2 | Blobs of A and B | IT-04-8 catalog cases | `BlobRef`; `None`; `None` (full ST-04-5 in E-10/c14) | NFT-SEC-13 |
| AC-3 | Three blob states | IT-04-8 reinstate cases | `Reinstated`, `BeyondRetention`, `FileMissing` | FT-P-18 |
| AC-4 | Retention 7 d and 0, `Due::Now`, rollback, duplicate | IT-04-9 | Due times, both rows or neither, one row kept | FT-P-16, FT-P-17 |
| AC-5 | Queued blobs of A and B | IT-04-9 pending count | Exact count for A | |

## Constraints

- Every registry and queue write runs in the caller's transaction and opens none; the file check of `reinstate_in_tx` is a stat, no database transaction of 04's own spans it.
- `BlobRef` is defined by 04 only; the media type of a download is not part of it.

## Risks & Mitigation

**Risk 1: Queue and state drift apart**
- *Risk*: a second code path sets `gc_pending` without a queue row.
- *Mitigation*: `schedule_in_tx` is the single writer of both (one repository function); IT-04-9 checks rollback and duplicates.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None open | - | - | - |

## Contract

This task produces/implements the registry, catalog and queue parts of the contract at `_docs/02_document/contracts/04_blobstore/blob-registry-gc.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
