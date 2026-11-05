# UploadService stage_small for files up to 2 MiB, zero bytes included

**Task**: AZ-3737_transfer_stage_small
**Name**: Transfer stage_small
**Description**: Accept a whole file of 0 to 2 MiB in one idempotent request: pre-check key, caps and quota before writing, write and sync, then record a completed staged row and reserve quota, discarding the file if anything fails after the write
**Complexity**: 3 points
**Dependencies**: AZ-3733_transfer_skeleton, AZ-3735_transfer_put_block, AZ-3766_sc_ids, AZ-3726_blob_writer, AZ-3729_blob_registry_gc_queue, AZ-3750_quota_ledger, AZ-3751_quota_admin_lifecycle
**Component**: 03_transfer
**Tracker**: AZ-3737
**Epic**: AZ-3643 (E-14)
**Ref**: E-14/c07

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/03_transfer/upload_service.md` — this task adds `stage_small`.

## Problem

AC-40 asks for 10,000 small files in 60 s at 20 ms round-trip time, which only works if each small file is one request and many are committed in one batch. A refused request must write nothing (quota and staged-cap refusals before any byte), a retry must not store the file twice, and an empty file must be a real file.

## Outcome

- `stage_small(staged_key, expected_hash?, body, length 0..=2 MiB)` returns `Staged{staged_key, size}` and is idempotent by key.
- Refusals for caps, quota and floor happen before any byte is written.
- A zero-byte file becomes a staged row with `block_count = 0`, `declared_size = 0` and an empty blob.

## Scope

### Included
- Order (03 §3): (1) look up `(account, staged_key)`: an existing row answers `Staged` without writing (same size and hash) or `ClientKeyMismatch`; (2) pre-check `StagedCapReached` (`max_staged`), the account in-flight cap (`Overloaded`) and free quota via `QuotaAdmin::view`; free-space floor (`FreeSpaceFloor`); (3) `BlobWriter::write_complete` (no block for length 0); (4) one transaction: `QuotaLedger::reserve`, `blobs` `uploading`, completed `upload_sessions` row (`block_count` 0 or 1, bits set).
- Hash check of the body (`HashMismatch`); `expected_hash` omitted when `length = 0`; `LengthMismatch` for a body not equal to `length`.
- Compensating `BlobWriter::discard` when the transaction refuses or fails.
- `AccountNotActive`, `Storage`.

### Excluded
- Batch commit of staged rows (E-14/c09); staged expiry (E-14/c11).

## Acceptance Criteria

**AC-1: Sizes and the empty file (AC-40)**
Given lengths 0, 1, 2 MiB − 1 and 2 MiB
When each is staged
Then each returns `Staged{size}`; the 0-byte row has `block_count = 0`, `declared_size = 0` and an empty blob; the others have `block_count = 1`

**AC-2: Above 2 MiB refused**
Given a body of 2 MiB + 1
When it is staged
Then a typed error is returned with no row and no file

**AC-3: Idempotent by key**
Given a staged file
When the same key is sent again with the same hash and size, and with another hash
Then the first returns `Staged` with no write, and the second returns `ClientKeyMismatch`

**AC-4: Refusals before any write (AC-39, AC-40)**
Given a wrong `expected_hash`, an account with 1,000 staged rows, an exhausted quota and free space below the floor, in separate runs
When a file is staged in each
Then they return `HashMismatch`, `StagedCapReached`, `QuotaExceeded` and `FreeSpaceFloor`, the blob directory file count is unchanged and no row exists

**AC-5: Compensation after the write**
Given a decorator that makes the reservation fail after `write_complete` returned
When a file is staged
Then the file is discarded, 0 rows remain and reserved bytes are unchanged

## Non-Functional Requirements

**Performance**
- `stage_small` p95 at most 40 ms (fail 100 ms) at concurrency 16; measured by PT-03-3 in E-14/c14.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | `block_count` for 0, 1 and 2 MiB | 0, 1, 1 |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | DS-23 sizes | Stage 0, 1, 2 MiB−1, 2 MiB (IT-03-5) | `Staged`; empty-file row | FT-P-04 |
| AC-2 | 2 MiB + 1 | Stage (IT-03-5) | Typed error, nothing stored | |
| AC-3 | Staged key | Repeat; other hash (IT-03-5) | No write; `ClientKeyMismatch` | FT-N-38 |
| AC-4 | Four refusal setups | Stage (IT-03-5) | Typed refusals; file count unchanged | NFT-RES-LIM-01 |
| AC-5 | Failing reservation | Stage (IT-03-5) | Discarded; 0 rows | AC-68 |

## Constraints

- Real 04 writer on a temp directory and real 07a; failures injected by a decorator over the real port.

## Risks & Mitigation

**Risk 1: Staged cap throttles bulk imports (R29)**
- *Risk*: 1,000 staged rows per account slow a large import.
- *Mitigation*: `max_staged_per_account` is published to clients (11's `ServerInfo`); clients pace batches.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The error for a body above 2 MiB is not named (IT-03-5 says "LengthMismatch or size error") | E-14 owner | open | Low |
| `AccountNotActive` for staging needs the `AccountGuard` port, which 03's Ports table does not list (see E-14/c04) | E-14 owner, E-00 | open | Medium |
