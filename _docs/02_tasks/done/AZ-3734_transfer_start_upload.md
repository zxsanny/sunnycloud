# UploadService start_upload with resume, caps and compensation

**Task**: AZ-3734_transfer_start_upload
**Name**: Transfer start_upload
**Description**: Start or resume a large upload session: name and parent checks, the `NothingToSend` shortcut, free-space floor, quota reservation and session cap in one short transaction, file creation after it, and a compensating transaction on failure
**Complexity**: 5 points
**Dependencies**: AZ-3732_transfer_spike_nothing_to_send, AZ-3733_transfer_skeleton, AZ-3707_tree_reader, AZ-3766_sc_ids, AZ-3767_sc_names, AZ-3726_blob_writer, AZ-3729_blob_registry_gc_queue, AZ-3750_quota_ledger, AZ-3751_quota_admin_lifecycle
**Component**: 03_transfer
**Tracker**: AZ-3734
**Epic**: AZ-3643 (E-14)
**Ref**: E-14/c04

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` — port signatures and DTOs produced by E-00/c04.
- `_docs/02_document/contracts/03_transfer/upload_service.md` — produced by this task (E-14/c05, c06, c07, c11 extend it).
- `_docs/02_document/contracts/02_tree/tree_reader.md` — `get_node`, `find_live_node`.

## Problem

A large upload must reserve its quota before any byte is written (RESTRICT-09), never exceed the per-account session cap or push free disk below the floor (AC-39), and resume the same session when the same file is selected again (AC-50). A client that lost a commit response must learn that the file is already there instead of sending it again. A start that fails after reserving must leave no rows and no reservation behind.

## Outcome

- `start_upload` returns `Started{upload_id, need_blocks, block_size = 2 MiB, lane_limit, expires_at}` or `NothingToSend{revision}`, or a typed refusal that reserves nothing.
- The same `(account, client_key)` returns the existing session; a changed declared size is `ClientKeyMismatch`.
- 16 concurrent starts against a cap of 8 yield exactly 8 sessions.

## Scope

### Included
- Order: size cap (`SizeTooLarge` above `max_declared_size`), name rules (helper 06, `InvalidName`), parent check via `TreeReader::get_node` (`ParentNotFound`, `NotAFolder`), the `NothingToSend` shortcut via `TreeReader::find_live_node` per the E-14/c02 rule, free-space floor (`FreeSpaceFloor`), blob plan, then one short transaction: `QuotaLedger::reserve` (takes the quota row lock), open-session cap check (`SessionCapReached`, rolls the reservation back), `blobs` row `uploading` and the `upload_sessions` row.
- File creation after the transaction; on failure a compensating short transaction removes both rows and releases the reservation.
- Resume by `(account, client_key)`; `ClientKeyMismatch` when the declared size differs.
- `AccountNotActive` for an account in `deleting` or revoked.
- Metrics `upload_sessions_active`, `upload_rejections_total{reason}`; WARN on quota, cap and floor refusals (ids only).

### Excluded
- Blocks, resume state reads and abort (E-14/c05, c06); staged small files (E-14/c07).

## Acceptance Criteria

**AC-1: Started for a valid file**
Given quota 10 GiB and a folder
When a 10 MiB file is started
Then the answer is `Started` with `need_blocks` 0..4, `block_size` 2 MiB, `lane_limit` 4 and an `expires_at`, and reserved bytes rose by 10 MiB

**AC-2: Resume by client key (AC-50)**
Given the AC-1 session
When start is repeated with the same `client_key`, with the same key and another declared size, and with a different key
Then the first returns the same `upload_id`, the second `ClientKeyMismatch`, the third a new session

**AC-3: Refusals reserve nothing (AC-39)**
Given quota 10 GiB, 8 open sessions and the floor probe below the floor in separate runs
When files of 51 GiB, 11 GiB, a 9th session and a start below the floor are requested
Then they return `SizeTooLarge`, `QuotaExceeded`, `SessionCapReached` and `FreeSpaceFloor`, and reserved bytes are unchanged after each

**AC-4: Cap serialised by the quota lock**
Given `max_open_sessions = 8` and no open session
When 16 starts run concurrently
Then exactly 8 sessions exist and 8 starts return `SessionCapReached`

**AC-5: Name and parent checks (AC-42)**
Given the name `a/b`, a missing parent and a file as parent
When each start runs
Then they return `InvalidName`, `ParentNotFound` and `NotAFolder` with nothing reserved

**AC-6: NothingToSend shortcut**
Given an existing file whose current revision has content hash h
When start is called for its parent and name with `content_hash = h`
Then the answer is `NothingToSend{revision}` with 0 bytes to send and nothing reserved, and every other case of the E-14/c02 rule gives its recorded outcome

**AC-6b: Committed key answers NothingToSend (lost-commit recovery, owner decision 2026-11-13)**
Given an upload with `client_key` k committed (its commit response lost), after which the name holds another file or the file was moved or deleted
When start is called again with k within `upload_session_ttl`
Then the answer is `NothingToSend{revision}` = the revision k's commit created, nothing reserved and no new session; after the TTL k starts a new session (`upload_service.md` 1.2.0)

**AC-7: Compensation after a failed file create**
Given file creation is made to fail after the transaction
When start runs
Then 0 session rows and 0 blob rows remain and reserved bytes are unchanged

**AC-8: Account not active**
Given an account in state `deleting`
When start runs
Then it returns `AccountNotActive` and reserves nothing

## Non-Functional Requirements

**Reliability**
- The reservation happens before any file exists; no long transaction spans file I/O.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | `need_blocks` and `block_count` for sizes around 2 MiB multiples | `ceil(size / 2 MiB)` blocks |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Quota 10 GiB | Start 10 MiB (IT-03-1) | `Started`; reserved +10 MiB | |
| AC-2 | Open session | Same key, changed size, new key (IT-03-1) | Same id; `ClientKeyMismatch`; new | FT-P-67, FT-N-03 |
| AC-3 | Caps and floor | 51 GiB, 11 GiB, 9th, floor (IT-03-1) | Typed refusals; reserved unchanged | NFT-RES-05, NFT-RES-LIM-01 |
| AC-4 | Cap 8 | 16 concurrent starts (IT-03-1) | Exactly 8 sessions | AC-39 |
| AC-5 | Bad name and parents | Start (IT-03-1) | `InvalidName`, `ParentNotFound`, `NotAFolder` | FT-N-04 |
| AC-6 | File with hash h | Start with h (IT-03-1) | `NothingToSend` | IT-13-16 |
| AC-7 | Injected create failure | Start (IT-03-1) | 0 rows; reserved unchanged | AC-68 |
| AC-8 | Account `deleting` | Start (IT-03-1) | `AccountNotActive` | IT-03-15 |

## Constraints

- Real 02 `TreeReader`, 04 `BlobWriter`/`BlobRegistry` on a temp directory and 07a on one PostgreSQL; fault injection by a thin decorator that delegates to the real `BlobWriter`.
- Until E-15 lands, the account-state check uses a port-level test implementation; the real one runs in E-14/c15.

## Risks & Mitigation

**Risk 1: Disk filled by declared sizes (R13)**
- *Risk*: many large declarations reserve more than the disk holds.
- *Mitigation*: quota and session cap in one transaction; floor checked before planning; ST-03-5 in E-14/c13.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| 03's Ports table and `module-layout.md` do not list `AccountGuard`, yet `AccountNotActive` is required at start, put and stage; a `contracts`/ports change is needed (Layout Rule 6) | E-14 owner, E-00 | resolved 2026-11-13 (owner): 03 takes `Arc<dyn AccountGuard>` like 02 (additive) | Medium |
| `data_model.md` §6.3 counts open sessions as rows with `block_count > 1`, so a large-path session of at most 2 MiB is not counted against the cap | E-14 owner | resolved 2026-11-13 (owner): every open large-path session counts toward the cap | Low |
| `declared_size = 0` on the large path (ST-03-2) has no named error | E-14 owner | resolved 2026-11-13 (owner): a zero-byte file never takes the large path; the large path refuses it with the existing validation error | Low |
| Free-space floor default is not fixed (G-R9; E-31/c04) | Owner | resolved 2026-11-13 (owner): the floor is the smaller of 10 GiB and 5% of the data volume (04 `blob_free_reserve` 10 GiB cap plus a 5% share) | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/03_transfer/upload_service.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
