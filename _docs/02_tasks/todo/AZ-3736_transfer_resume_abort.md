# Resume after drop and restart, get_upload and abort_upload

**Task**: AZ-3736_transfer_resume_abort
**Name**: Transfer resume and abort
**Description**: Report session state with `get_upload`, rebuild progress lazily from the last checkpoint after a restart, and abort a session releasing exactly its reservation and file
**Complexity**: 3 points
**Dependencies**: AZ-3731_transfer_spike_resend_stale_base, AZ-3734_transfer_start_upload, AZ-3735_transfer_put_block, AZ-3729_blob_registry_gc_queue, AZ-3750_quota_ledger
**Component**: 03_transfer
**Tracker**: AZ-3736
**Epic**: AZ-3643 (E-14)
**Ref**: E-14/c06

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/03_transfer/upload_service.md` — this task adds `get_upload` and `abort_upload`.

## Problem

An interrupted upload must resume and resend at most 10 MiB (AC-3). After a connection drop only the in-flight blocks are missing; after a server restart the progress table is empty and must be rebuilt from the database checkpoint on first use, not at start (no rehydrate). A user who cancels must get the reserved quota back exactly once.

## Outcome

- `get_upload` returns `UploadState{need_blocks, received_count, expires_at}` or `UploadNotFound` (also when expired).
- After a restart the first access loads the session from its checkpoint; acknowledged but uncheckpointed blocks are listed again.
- `abort_upload` deletes the row, releases exactly its `reserved_bytes`, removes the blob row and discards the file after commit.

## Scope

### Included
- `get_upload(upload_id)` scoped by account.
- Lazy load of the progress entry from `received` and `block_hashes` on first access after a restart.
- `abort_upload(upload_id)`: one short transaction (`DELETE … RETURNING blob_id, reserved_bytes`, `QuotaLedger::release_reservation` for the returned amount, `BlobRegistry::remove_row_in_tx`), file discarded after COMMIT, progress entry evicted; `upload_sessions_total{result = aborted}`.
- Resend measurement for AC-3 using the bound recorded by E-14/c01.

### Excluded
- The reaper and idle flush (E-14/c11); session start (E-14/c04).

## Acceptance Criteria

**AC-1: Drop resends only in-flight blocks (AC-3)**
Given a 100 MiB upload over 4 lanes
When the 4 in-flight bodies are cut mid-stream
Then `get_upload.need_blocks` lists only those blocks, the resend is at most 8 MiB, and the progress entry is intact

**AC-2: Restart loads from the checkpoint (AC-3)**
Given 30 blocks acked with `checkpoint_bytes` 8 MiB and `checkpoint_interval` 1 s, and the service dropped right after the last ack and before its checkpoint
When a new service instance serves `get_upload`
Then the session is loaded from the checkpoint row, acked-but-uncheckpointed blocks are listed again, the measured resend is recorded, and it is at most 10 MiB or at most the bound E-14/c01 recorded

**AC-3: Abort releases exactly once (AC-74)**
Given an open session with 10 MiB reserved and 2 blocks written
When `abort_upload` runs, and runs again
Then the row, blob row and file are gone, reserved bytes fell by exactly 10 MiB, and the second call returns `UploadNotFound` and releases nothing

**AC-4: Expired and foreign sessions**
Given an expired session and another account's session
When `get_upload` is called for each
Then both return `UploadNotFound`

## Non-Functional Requirements

**Reliability**
- No progress state is loaded at start; a session is loaded only when used.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Rebuild of bits and hashes from a checkpoint row | Matches the checkpoint |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 100 MiB, 4 lanes | Cut in-flight bodies (IT-03-4 case A) | Only in-flight missing; <= 8 MiB | FT-P-01 |
| AC-2 | 30 acked, kill before checkpoint | New instance (IT-03-4 case B) | Lazy load; resend <= bound | NFT-PERF-05, NFT-RES-01 |
| AC-3 | 10 MiB reserved | Abort twice (IT-03-10 abort line) | Exact release once | NFT-RES-38 |
| AC-4 | Expired; foreign | `get_upload` | `UploadNotFound` | ST-03-1 |

## Constraints

- Case B of IT-03-4 may legitimately fail the 10 MiB assertion until G-03-1 is decided (03 tests.md note); the result is reported, never relaxed silently.
- The acceptance run AT-03-1 needs a commit and is part of E-14/c08.

## Risks & Mitigation

**Risk 1: Double release with reaper or commit**
- *Risk*: abort, reaper and commit each release the reservation.
- *Mitigation*: whoever deletes the row releases; AC-3 checks the repeat.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-03-1: worst case 16 MiB against AC-3's 10 MiB; AC-2's bound comes from E-14/c01 | Owner | open | Medium |
